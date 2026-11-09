import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import App from "./App";
import { Popup } from "./Popup";
import { FormWindow } from "./Form";
import "./styles.css";

// One bundle, three windows: the main editor, the search popup and the fill-in form.
const label = getCurrentWebviewWindow().label;
const Root = label === "popup" ? Popup : label === "form" ? FormWindow : App;

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Root />
  </StrictMode>,
);
