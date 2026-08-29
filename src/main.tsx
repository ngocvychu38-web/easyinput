import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { VoiceOverlay } from "./components/VoiceOverlay";
import "./styles.css";
import "./realtime.css";
import "./dictionary.css";
import "./overview-extra.css";
import "./keyboard.css";
import "./app-picker.css";
import "./voice-overlay.css";

const voiceOverlay = new URLSearchParams(window.location.search).get("window") === "voice-overlay";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>{voiceOverlay ? <VoiceOverlay /> : <App />}</React.StrictMode>
);
