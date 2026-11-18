import { createHmac, webcrypto } from "node:crypto";
import { readFile } from "node:fs/promises";
import { setTimeout as wait } from "node:timers/promises";
import assert from "node:assert/strict";
import { test } from "node:test";

if (!globalThis.crypto) globalThis.crypto = webcrypto;

const rule = "https://example.invalid/file.zip";
const origin = "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/";
const profile = "a".repeat(64);
const captures = [];
const storage = { repeatableUrl: rule };
const downloads = new Map();
let prepareMode = "durable";
let commitMode = "success";
const committed = new Set();
const events = {};
const event = name => ({ addListener: callback => { events[name] = callback; } });

globalThis.chrome = {
  storage: {
    local: {
      setAccessLevel: async () => {},
      get: async key => ({ [key]: storage[key] }),
      set: async values => Object.assign(storage, values),
    },
  },
  downloads: {
    onCreated: event("created"),
    search: async ({id}) => [{ ...downloads.get(id) }],
    cancel: async id => {
      captures.push("cancel");
      Object.assign(downloads.get(id), { state: "interrupted", error: "USER_CANCELED" });
    },
  },
  webRequest: { onBeforeSendHeaders: event("headers") },
  contextMenus: { onClicked: event("clicked") },
  runtime: {
    id: origin.split("/")[2],
    getURL: path => origin + path,
    onInstalled: event("installed"),
    onStartup: event("startup"),
    onMessage: event("message"),
    connectNative: () => {
      let listener;
      let key;
      return {
        onMessage: { addListener: callback => { listener = callback; } },
        onDisconnect: { addListener: () => {} },
        disconnect: () => {},
        postMessage: req => {
          const {payload} = req;
          let result = { request_id: req.request_id, status: "ok" };
          switch (req.op) {
            case "hello":
              if (payload.stage === "enroll") {
                key = Buffer.from(payload.installation_key, "hex");
                result.data = { registered_instance_id: profile };
              } else if (payload.stage === "challenge") {
                result.data = { challenge_id: "challenge-id", challenge: "server-nonce" };
              } else if (payload.stage === "prove") {
                const expected = createHmac("sha256", key)
                  .update("rustcatch-hello-v1:challenge-id:server-nonce:" + origin + ":" + profile).digest("hex");
                assert.equal(payload.auth_proof, expected);
              }
              break;
            case "handoff.status":
              captures.push("status");
              if (committed.has(req.idempotency_key)) {
                result = { ...result, durable: true, data: { handoff_state: "committed" } };
              } else result.status = "error";
              break;
            case "handoff.prepare":
              captures.push("prepare");
              result = {
                ...result, status: "prepared", durable: prepareMode === "durable",
                task_id: "task-1", revision: "1", handoff_token: "fixture-token",
                expires_at: new Date(Date.now() + 120000).toISOString(),
              };
              break;
            case "handoff.commit":
              captures.push("commit");
              assert.equal(payload.browser_cancel_confirmed, true);
              assert.equal(downloads.get(payload.browser_download_id).error, "USER_CANCELED");
              committed.add(req.idempotency_key);
              result = { ...result, status: commitMode === "success" ? "queued" : "error", durable: commitMode === "success" };
              break;
            default:
              assert.fail("Unexpected bridge operation");
          }
          queueMicrotask(() => listener(result));
        },
      };
    },
  },
};

const source = await readFile(new URL("../extension/background.js", import.meta.url));
const { qualifiedGet, cancellationConfirmed } = await import("data:text/javascript;base64," + source.toString("base64"));

async function trigger(id, options = {}) {
  const item = {
    id, state: "in_progress", url: rule, finalUrl: rule, startTime: new Date().toISOString(),
    referrer: "https://example.invalid/", filename: "file.zip", danger: "safe",
  };
  downloads.set(id, item);
  events.headers({
    url: rule, tabId: 4, frameId: 0, initiator: "https://example.invalid",
    method: "GET", requestId: "req-" + id, timeStamp: Date.now(),
    requestHeaders: [], ...options,
  });
  await wait(5);
  events.created({ ...item });
}

async function until(predicate) {
  for (let i = 0; i < 100; i++) {
    if (predicate()) return;
    await wait(5);
  }
  assert.fail("handoff fixture did not reach expected state");
}

test("keeps POST, redirected, credentialed, and ambiguous requests in the browser", () => {
  const now = Date.now();
  const item = { state: "in_progress", startTime: new Date(now).toISOString(), url: rule,
    finalUrl: rule, referrer: "https://example.invalid/" };
  const seen = { method: "GET", time: now, origin: "https://example.invalid", private: false };
  assert.equal(qualifiedGet(item, rule, [seen], now), seen);
  assert.equal(qualifiedGet(item, rule, [{ ...seen, method: "POST" }], now), null);
  assert.equal(qualifiedGet(item, rule, [{ ...seen, private: true }], now), null);
  assert.equal(qualifiedGet(item, rule, [seen, seen], now), null);
  assert.equal(qualifiedGet({ ...item, finalUrl: "https://another.invalid/file.zip" }, rule, [seen], now), null);
  assert.equal(cancellationConfirmed({state: "interrupted", error: "NETWORK_FAILED"}), false);
});

test("cancels only after durable prepare and recovers a missing commit ACK without canceling twice", async () => {
  prepareMode = "not-durable";
  await trigger(1);
  await until(() => storage.bridgeStatus?.includes("未确认可接管"));
  assert.deepEqual(captures, ["status", "prepare"]);
  assert.equal(downloads.get(1).state, "in_progress");

  captures.length = 0;
  storage.handoffOutbox = {};
  prepareMode = "durable";
  commitMode = "lost";
  await trigger(2);
  await until(() => storage.bridgeStatus?.includes("桌面尚未确认"));
  assert.deepEqual(captures, ["status", "prepare", "cancel", "commit"]);
  assert.equal(Object.values(storage.handoffOutbox)[0].phase, "cancel_sent");
  assert.ok(!JSON.stringify(storage.handoffOutbox).includes("fixture-token"));

  captures.length = 0;
  await new Promise(resolve => events.message({ type: "rustcatch.retry-handoff" },
    { id: chrome.runtime.id, url: origin + "popup.html" }, resolve));
  assert.deepEqual(captures, ["status"]);
  assert.deepEqual(storage.handoffOutbox, {});
});
