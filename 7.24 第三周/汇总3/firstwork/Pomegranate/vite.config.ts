import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "path";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

function manualChunks(id: string) {
  const normalized = id.replace(/\\/g, "/");
  if (!normalized.includes("/node_modules/")) return undefined;

  if (
    normalized.includes("/node_modules/react/") ||
    normalized.includes("/node_modules/react-dom/") ||
    normalized.includes("/node_modules/react-router-dom/")
  ) {
    return "vendor-react";
  }

  if (
    normalized.includes("/node_modules/@tiptap/") ||
    normalized.includes("/node_modules/prosemirror-") ||
    normalized.includes("/node_modules/lowlight/")
  ) {
    return "vendor-editor";
  }
  if (normalized.includes("/node_modules/@codemirror/")) return "vendor-codemirror";
  if (normalized.includes("/node_modules/cytoscape/")) return "vendor-cytoscape";
  if (normalized.includes("/node_modules/@antv/")) return "vendor-antv";
  if (normalized.includes("/node_modules/markmap-")) return "vendor-markmap";
  if (
    normalized.includes("/node_modules/react-markdown/") ||
    normalized.includes("/node_modules/remark-") ||
    normalized.includes("/node_modules/rehype-") ||
    normalized.includes("/node_modules/unified/") ||
    normalized.includes("/node_modules/micromark") ||
    normalized.includes("/node_modules/mdast-") ||
    normalized.includes("/node_modules/hast-")
  ) {
    return "vendor-markdown";
  }
  if (
    normalized.includes("/node_modules/html5-qrcode/") ||
    normalized.includes("/node_modules/qrcode/")
  ) {
    return "vendor-qrcode";
  }
  if (normalized.includes("/node_modules/@tauri-apps/")) return "vendor-tauri";
  if (normalized.includes("/node_modules/lucide-react/")) return "vendor-icons";
  if (normalized.includes("/node_modules/mammoth/")) return "vendor-importers";
  if (normalized.includes("/node_modules/katex/")) return "vendor-katex";

  return undefined;
}

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react(), tailwindcss()],

  resolve: {
    alias: {
      "@": path.resolve(__dirname, "src"),
    },
  },

  build: {
    // 生产构建优化
    // ⚠️ macOS 12 Monterey 自带 WKWebView = Safari 15.6，不支持 ES2023
    // (Array.findLast / toSorted / toReversed 等)。esnext 不做任何转译，
    // 一旦项目依赖（tiptap 3 / antd 6 / lucide 等）输出 ES2023 语法就会
    // 抛 SyntaxError 让整个 chunk 加载失败 → 白屏。
    // 把 target 限到 safari15 让 esbuild 把 ES2023 降级到 ES2020 兼容代码。
    // chrome88/edge88/firefox88 是 antd 6 官方最低门槛，对齐避免漏网。
    target: ["es2020", "safari15", "chrome88", "edge88", "firefox88"],
    minify: "terser",
    terserOptions: {
      compress: { drop_console: true, drop_debugger: true },
    },
    rollupOptions: {
      output: {
        manualChunks,
      },
    },
    chunkSizeWarningLimit: 1500,
  },

  clearScreen: false,
  server: {
    port: 2010,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 2020,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
}));
