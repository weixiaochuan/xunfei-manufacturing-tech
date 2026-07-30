"""Domain models for Manufacturing — auto-generated from ontology."""

from __future__ import annotations

from datetime import date, datetime
from enum import Enum
from typing import Literal

from pydantic import BaseModel, Field

class Person(BaseModel):
    """Entity model for Person."""

    name: str = Field(...)
    email: str | None = None
    role: str | None = None
    description: str | None = None

class Organization(BaseModel):
    """Entity model for Organization."""

    name: str = Field(...)
    description: str | None = None
    industry: str | None = None

class Location(BaseModel):
    """Entity model for Location."""

    name: str = Field(...)
    address: str | None = None
    latitude: float | None = None
    longitude: float | None = None

class Event(BaseModel):
    """Entity model for Event."""

    name: str = Field(...)
    date: datetime | None = None
    description: str | None = None

class Object(BaseModel):
    """Entity model for Object."""

    name: str = Field(...)
    description: str | None = None

class MachineMachineTypeEnum(str, Enum):
    CNC = "cnc"
    PRESS = "press"
    LATHE = "lathe"
    WELDER = "welder"
    ASSEMBLER = "assembler"
    CONVEYOR = "conveyor"
    ROBOT = "robot"
    PACKAGING = "packaging"

class MachineStatusEnum(str, Enum):
    OPERATIONAL = "operational"
    MAINTENANCE = "maintenance"
    OFFLINE = "offline"
    DECOMMISSIONED = "decommissioned"

class Machine(BaseModel):
    """Entity model for Machine."""

    machine_id: str = Field(...)
    name: str = Field(...)
    machine_type: MachineMachineTypeEnum | None = None
    manufacturer: str | None = None
    install_date: date | None = None
    status: MachineStatusEnum | None = None
    operating_hours: int | None = None

class PartCategoryEnum(str, Enum):
    RAW_MATERIAL = "raw_material"
    COMPONENT = "component"
    SUBASSEMBLY = "subassembly"
    FINISHED_GOOD = "finished_good"

class Part(BaseModel):
    """Entity model for Part."""

    part_number: str = Field(...)
    name: str = Field(...)
    category: PartCategoryEnum | None = None
    unit_cost: float | None = None
    stock_quantity: int | None = None
    minimum_stock: int | None = None
    specification: str | None = None

class WorkOrderPriorityEnum(str, Enum):
    LOW = "low"
    MEDIUM = "medium"
    HIGH = "high"
    CRITICAL = "critical"

class WorkOrderStatusEnum(str, Enum):
    PLANNED = "planned"
    IN_PROGRESS = "in_progress"
    COMPLETED = "completed"
    ON_HOLD = "on_hold"
    CANCELLED = "cancelled"

class WorkOrder(BaseModel):
    """Entity model for WorkOrder."""

    work_order_id: str = Field(...)
    description: str = Field(...)
    priority: WorkOrderPriorityEnum | None = None
    status: WorkOrderStatusEnum | None = None
    start_date: datetime | None = None
    due_date: datetime | None = None
    quantity: int = Field(...)

class SupplierStatusEnum(str, Enum):
    APPROVED = "approved"
    PROBATIONARY = "probationary"
    SUSPENDED = "suspended"
    BLACKLISTED = "blacklisted"

class Supplier(BaseModel):
    """Entity model for Supplier."""

    supplier_id: str = Field(...)
    name: str = Field(...)
    country: str | None = None
    lead_time_days: int | None = None
    quality_rating: float | None = None
    status: SupplierStatusEnum | None = None

class QualityReportResultEnum(str, Enum):
    PASS = "pass"
    FAIL = "fail"
    CONDITIONAL_PASS = "conditional_pass"

class QualityReport(BaseModel):
    """Entity model for QualityReport."""

    report_id: str = Field(...)
    inspection_date: datetime = Field(...)
    inspector: str | None = None
    result: QualityReportResultEnum | None = None
    defect_count: int | None = None
    defect_type: str | None = None
    notes: str | None = None

class ProductionLineStatusEnum(str, Enum):
    ACTIVE = "active"
    IDLE = "idle"
    MAINTENANCE = "maintenance"
    RETOOLING = "retooling"

class ProductionLineShiftPatternEnum(str, Enum):
    SINGLE = "single"
    DOUBLE = "double"
    TRIPLE = "triple"
    CONTINUOUS = "continuous"

class ProductionLine(BaseModel):
    """Entity model for ProductionLine."""

    line_id: str = Field(...)
    name: str = Field(...)
    capacity_per_hour: int | None = None
    efficiency_rating: float | None = None
    status: ProductionLineStatusEnum | None = None
    shift_pattern: ProductionLineShiftPatternEnum | None = None

