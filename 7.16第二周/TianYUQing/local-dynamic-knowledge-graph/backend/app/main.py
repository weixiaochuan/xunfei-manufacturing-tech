"""Manufacturing Context Graph — FastAPI Application."""

import logging
import os
from contextlib import asynccontextmanager

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import FileResponse
from fastapi.staticfiles import StaticFiles
from pathlib import Path

from app.config import settings
from app.context_graph_client import connect_neo4j, close_neo4j, is_connected, execute_cypher
from app.memory import (
    close_memory,
    connect_memory,
    get_client,
    get_error_category,
    get_error_detail,
    get_error_message,
)
from app.routes import router

logger = logging.getLogger(__name__)

# Backend connection state
_neo4j_available = False
_memory_available = False


@asynccontextmanager
async def lifespan(app: FastAPI):
    """Manage application lifecycle.

    On the NAMS backend, we only initialize the memory client — there is no
    bolt Neo4j to connect to. On the self-hosted bolt backend, we connect to
    Neo4j (which in turn initializes the memory integration).
    """
    global _neo4j_available, _memory_available

    if settings.memory_backend == "nams":
        try:
            await connect_memory()
            _memory_available = get_client() is not None
            if _memory_available:
                logger.info("NAMS memory client connected")
            else:
                # connect_memory() already logged the classified error.
                # Re-state it here so the startup banner is self-contained.
                msg = get_error_message() or "NAMS memory client unavailable"
                logger.warning("Starting in degraded mode: %s", msg)
        except Exception as e:
            _memory_available = False
            logger.warning("NAMS unavailable — starting in degraded mode: %s", e)
    else:
        try:
            await connect_neo4j()
            _neo4j_available = True
            _memory_available = True
            logger.info("Neo4j connected successfully")
        except Exception as e:
            _neo4j_available = False
            _memory_available = False
            logger.warning("Neo4j unavailable — starting in degraded mode: %s", e)

        if _neo4j_available:
            try:
                from app.vector_client import create_vector_index
                await create_vector_index()
            except Exception as e:
                logger.warning("Vector index creation failed (non-fatal): %s", e)

    yield

    if settings.memory_backend == "nams":
        if _memory_available:
            await close_memory()
    else:
        if _neo4j_available:
            await close_neo4j()


def get_neo4j_status() -> bool:
    """Check if Neo4j is available (bolt backend only)."""
    return _neo4j_available


def get_memory_status() -> bool:
    """Check if the memory backend is available."""
    return _memory_available

app = FastAPI(
    title="Manufacturing Context Graph",
    description="Production management, quality control, supply chain, and equipment maintenance",
    version="0.1.0",
    lifespan=lifespan,
)


CORS_ORIGINS = os.getenv(
    "CORS_ORIGINS",
    f"http://localhost:{settings.frontend_port}",
).split(",")

app.add_middleware(
    CORSMiddleware,
    allow_origins=CORS_ORIGINS,
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)

app.include_router(router, prefix="/api")

# A dependency-free local web client is shipped with the Python backend.
# Keeping it here makes the demo runnable without Node.js or an internet CDN.
WEB_DIR = Path(__file__).resolve().parents[2] / "web"
app.mount("/static", StaticFiles(directory=WEB_DIR), name="static")


@app.get("/", include_in_schema=False)
async def local_graph_ui():
    return FileResponse(WEB_DIR / "index.html")


@app.get("/health")
async def health():
    """Health check endpoint with memory backend connectivity status."""
    if settings.memory_backend == "nams":
        memory_ok = get_memory_status()
        body = {
            "status": "ok" if memory_ok else "degraded",
            "memory_backend": "nams",
            "nams": memory_ok,
            "domain": "manufacturing",
            "version": "0.1.0",
        }
        if not memory_ok:
            category = get_error_category()
            if category:
                body["nams_error"] = category
                body["nams_error_message"] = get_error_message()
                body["nams_error_detail"] = get_error_detail()
            body["nams_dashboard"] = "https://memory.neo4jlabs.com"
        return body
    neo4j_ok = is_connected()
    if neo4j_ok:
        try:
            await execute_cypher("RETURN 1 AS ok", collect=False)
        except Exception:
            neo4j_ok = False
    return {
        "status": "ok" if neo4j_ok else "degraded",
        "memory_backend": "bolt",
        "neo4j": neo4j_ok,
        "domain": "manufacturing",
        "version": "0.1.0",
    }
