import React from "react";
import { createRoot } from "react-dom/client";
import { SharedMemorySettingsPanel } from "../../src/components/SharedMemorySettingsPanel";
import "../../src/styles.css";

createRoot(document.getElementById("root")!).render(<SharedMemorySettingsPanel />);
