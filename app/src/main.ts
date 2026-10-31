import { mount } from "svelte";

import App from "./App.svelte";
import "./app.css";
import { cspViolations } from "./lib/csp";

// Collected for the debug smoke run; a violation means the CSP blocks something the UI uses.
document.addEventListener("securitypolicyviolation", (event) => {
  cspViolations.push(`${event.violatedDirective} ${event.blockedURI}`);
});

const target = document.getElementById("app");
if (!target) throw new Error("missing #app");

export default mount(App, { target });
