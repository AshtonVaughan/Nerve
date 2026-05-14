import test from "node:test";
import assert from "node:assert/strict";

import { NerveClient, NerveClientError } from "../dist/index.js";

test("NerveClient constructs with defaults", () => {
  const c = new NerveClient();
  assert.ok(c);
});

test("NerveClient honours host / port options", () => {
  const c = new NerveClient({ host: "10.0.0.5", port: 9999 });
  assert.ok(c);
});

test("NerveClientError carries code and message", () => {
  const e = new NerveClientError("bad_request", "bad payload");
  assert.equal(e.code, "bad_request");
  assert.match(e.message, /bad_request/);
  assert.ok(e instanceof Error);
});
