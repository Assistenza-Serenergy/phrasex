// Screenshot mode only: events the real backend would send (open a phrase, open Settings).
const q = new URLSearchParams(location.search);
export async function listen(name: string, cb: (ev: { payload: unknown }) => void) {
  if (name === "open-entry" && q.get("entry")) setTimeout(() => cb({ payload: Number(q.get("entry")) }), 200);
  if (name === "open-settings" && q.get("view") === "settings") setTimeout(() => cb({ payload: null }), 200);
  if (name === "popup-show" && q.get("w") === "choose") setTimeout(() => cb({ payload: "choose" }), 200);
  return () => {};
}
