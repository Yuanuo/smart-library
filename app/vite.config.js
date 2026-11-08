import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { cpSync, existsSync, createReadStream, statSync } from "node:fs";
import { resolve, join, normalize } from "node:path";

// The whiteboard (Excalidraw) loads its fonts from a CDN by default. The
// program works offline, so they are served from here: the `fonts` folder of
// the package is `/excalidraw-assets/fonts/…` (set as EXCALIDRAW_ASSET_PATH).
const fonts = resolve("node_modules/@excalidraw/excalidraw/dist/prod/fonts");
function excalidrawFonts() {
  return {
    name: "excalidraw-fonts",
    configureServer(server) {
      server.middlewares.use("/excalidraw-assets/fonts", (req, res, next) => {
        const file = normalize(join(fonts, decodeURIComponent((req.url ?? "").split("?")[0])));
        if (!file.startsWith(fonts) || !existsSync(file) || !statSync(file).isFile()) return next();
        res.setHeader("Content-Type", "font/woff2");
        createReadStream(file).pipe(res);
      });
    },
    closeBundle() {
      if (existsSync(fonts)) cpSync(fonts, resolve("dist/excalidraw-assets/fonts"), { recursive: true });
    },
  };
}

// Tauri expects a fixed port and must see Rust errors in the terminal.
export default defineConfig({
  plugins: [svelte(), excalidrawFonts()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    target: "es2022",
  },
});
