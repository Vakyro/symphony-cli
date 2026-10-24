/* @refresh reload */
import { render } from "solid-js/web";
import App from "./App";
import "./styles.css";

const root = document.getElementById("root");

async function main() {
  // Solo en `pnpm dev` y sin Tauri: datos de ejemplo para revisar el diseño en un navegador.
  if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
    const [{ setTransport }, { demoTransport }] = await Promise.all([import("./ipc"), import("./demo")]);
    setTransport(demoTransport());
  }
  if (root) render(() => <App />, root);
}

void main();
