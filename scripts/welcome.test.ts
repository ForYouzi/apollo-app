import { expect, test } from "bun:test";
import { readFile } from "node:fs/promises";

// The welcome page's copyable migrate line names the command this OS has:
// the Windows zip ships only apollo-app.exe.
const source = (await readFile(new URL("../defaults/frame/welcome/main.js", import.meta.url), "utf8")).replace(/\r\n/g, "\n");
// Any element: every builder call returns itself.
const element = (): unknown => new Proxy(() => {}, { get: (_target, key) => key === Symbol.toPrimitive ? undefined : () => element() });

// What the "Coming from herdr?" card hands copyLine, as the person sees and copies it.
function copied(platform: string | null) {
  const values: Record<string, unknown> = {
    View: class {}, div: element, svg: element, image: element, v_flex: element, h_flex: element, Scrollbar: element,
    window: () => (platform ? { platform } : null),
  };
  const body = source.replace(/^import .*;\n/gm, "").replace("export default class Welcome", "return class Welcome");
  const Welcome = new Function(...Object.keys(values), body)(...Object.values(values));
  const view = new Welcome();
  const lines: [string, string][] = [];
  view.para = () => element();
  view.copyLine = (_t: unknown, text: string, id: string) => { lines.push([id, text]); return element(); };
  view.comingFromHerdr({});
  return lines;
}

test("the migrate line says apollo-app on Windows", () => {
  expect(copied("windows")).toEqual([["w-migrate-copy", "Read `apollo-app --skill`, then migrate my keybinds and settings from ~/.config/herdr/config.toml"]]);
});

test("the migrate line says herdr-app elsewhere and without a window", () => {
  for (const platform of ["macos", "linux", null]) {
    expect(copied(platform)).toEqual([["w-migrate-copy", "Read `herdr-app --skill`, then migrate my keybinds and settings from ~/.config/herdr/config.toml"]]);
  }
});
