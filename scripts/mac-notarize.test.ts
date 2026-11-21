import { test, expect } from "bun:test";
import { mkdtemp, writeFile, readFile, chmod, mkdir } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

const script = new URL("mac-notarize", import.meta.url).pathname;
const keys = { APPLE_API_KEY: "-----BEGIN PRIVATE KEY-----\nk\n-----END PRIVATE KEY-----", APPLE_API_KEY_ID: "KEYID", APPLE_API_ISSUER: "issuer" };

// Fake codesign, ditto and xcrun on PATH, logging each call; `status` is what Apple answers.
async function run(env: Record<string, string>, { signer = "Developer ID Application: Example (TEAM)", status = "Accepted" } = {}) {
  const dir = await mkdtemp(join(tmpdir(), "mac-notarize-"));
  const bin = join(dir, "bin");
  await mkdir(bin);
  const log = join(dir, "log");
  const fake = async (name: string, body: string) => {
    await writeFile(join(bin, name), `#!/bin/bash\necho "${name} $*" >> "${log}"\n${body}\n`);
    await chmod(join(bin, name), 0o755);
  };
  await fake("codesign", `echo "Authority=${signer}" >&2`);
  await fake("ditto", `touch "\${@: -1}"`);
  await fake("xcrun", `[[ $2 == submit ]] && echo '{"id":"sub-1","status":"${status}"}'; exit 0`);
  const app = join(dir, "Apollo.app");
  await mkdir(app);
  const p = Bun.spawnSync(["bash", script, app], { env: { PATH: `${bin}:/usr/bin:/bin`, ...env }, stderr: "pipe", stdout: "pipe" });
  const calls = (await readFile(log, "utf8").catch(() => "")).trim().split("\n").filter(Boolean);
  return { code: p.exitCode, calls, err: p.stderr.toString() };
}

test("without the notary key the app is left as signed", async () => {
  const r = await run({});
  expect(r.code).toBe(0);
  expect(r.calls).toEqual([]);
});

test("part of the notary key is an error, not a quiet unsigned release", async () => {
  const r = await run({ APPLE_API_KEY: keys.APPLE_API_KEY, APPLE_API_KEY_ID: keys.APPLE_API_KEY_ID });
  expect(r.code).not.toBe(0);
  expect(r.calls).toEqual([]);
});

test("the notary key on an ad hoc app is an error", async () => {
  const r = await run(keys, { signer: "" });
  expect(r.code).not.toBe(0);
  expect(r.calls.some(c => c.startsWith("xcrun"))).toBe(false);
});

test("an accepted submission is stapled to the app and validated", async () => {
  const r = await run(keys);
  expect(r.code).toBe(0);
  const xcrun = r.calls.filter(c => c.startsWith("xcrun"));
  expect(xcrun[0]).toMatch(/^xcrun notarytool submit \S+\/submit\.zip --key \S+ --key-id KEYID --issuer issuer --wait/);
  expect(xcrun.slice(1)).toEqual([expect.stringMatching(/^xcrun stapler staple \S+\/Apollo\.app$/), expect.stringMatching(/^xcrun stapler validate \S+\/Apollo\.app$/)]);
});

test("a rejected submission fails with Apple's log and staples nothing", async () => {
  const r = await run(keys, { status: "Invalid" });
  expect(r.code).not.toBe(0);
  expect(r.calls.some(c => c.startsWith("xcrun notarytool log sub-1"))).toBe(true);
  expect(r.calls.some(c => c.includes("stapler"))).toBe(false);
  expect(r.err).toContain("notarization Invalid");
});

test("the release zip is made after signing and notarizing, from the stapled app", async () => {
  const pkg = await readFile(new URL("package", import.meta.url), "utf8");
  const sign = pkg.indexOf('scripts/mac-sign "$app"');
  const notarize = pkg.indexOf('scripts/mac-notarize "$app"');
  const zip = pkg.indexOf('ditto -c -k --sequesterRsrc --keepParent "$app" "$out"');
  expect(sign).toBeGreaterThan(0);
  expect(notarize).toBeGreaterThan(sign);
  expect(zip).toBeGreaterThan(notarize);
});
