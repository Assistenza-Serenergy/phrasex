// Screenshot mode only: ?w=popup / ?w=choose picks the window to show.
const w = new URLSearchParams(location.search).get("w");
export const getCurrentWebviewWindow = () => ({ label: w === "choose" ? "popup" : w ?? "main", onFocusChanged: async () => () => {} });
