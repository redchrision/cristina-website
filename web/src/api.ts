/** Same-origin endpoints served by the Rust server (see /server). */
export const CONTACT_ENDPOINT = "/api/contact";

/**
 * Cookieless page-view beacon, OFF by default. Build with
 * `VITE_ANALYTICS=on npm run build` to enable it. It sends only the route,
 * the referrer host, the UI language and a coarse device class: no cookie,
 * no identifier, no IP stored server-side.
 *
 * Before enabling it, update the "Cookies" section of the legal page, which
 * currently states that no audience-measurement tool is installed.
 */
const ANALYTICS_ON = import.meta.env.VITE_ANALYTICS === "on";

function deviceClass(): string {
  const w = window.innerWidth;
  return w < 640 ? "mobile" : w < 1024 ? "tablet" : "desktop";
}

export function trackPageview(route: string, lang: string): void {
  if (!ANALYTICS_ON) return;
  if (navigator.doNotTrack === "1" || (navigator as { globalPrivacyControl?: boolean }).globalPrivacyControl) return;
  const body = JSON.stringify({
    path: "/" + route,
    referrer: document.referrer || undefined,
    lang,
    device: deviceClass(),
  });
  // text/plain keeps it a "simple" request: no CORS preflight.
  const blob = new Blob([body], { type: "text/plain" });
  if (!navigator.sendBeacon?.("/api/collect", blob)) {
    void fetch("/api/collect", { method: "POST", body, keepalive: true }).catch(() => {});
  }
}
