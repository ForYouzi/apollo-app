import { expect, test } from "bun:test";
import { openableUrl } from "../defaults/mods/win98/url.js";
test("only non-file urls open", () => {
  for (const u of ["https://a.com", "mailto:a@b.c"]) expect(openableUrl(u)).toBe(true);
  for (const u of ["file:///etc/hosts", "FILE:/x", "/etc/hosts", "C:\\x", "notes.txt", "", undefined]) expect(openableUrl(u)).toBe(false);
});
