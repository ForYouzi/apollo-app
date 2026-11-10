/** A menu item's url may open only with a real scheme, never `file:`. */
export function openableUrl(url) {
  const m = /^([A-Za-z][A-Za-z0-9+.-]*):/.exec(url ?? "");
  return !!m && m[1].length > 1 && m[1].toLowerCase() !== "file";
}
