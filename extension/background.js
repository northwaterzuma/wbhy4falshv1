// Browser downloads stay in Chromium unless this profile explicitly opted in to replay the exact URL.
const NATIVE_HOST = "com.rustcatch.bridge";
const MAX_REQUEST_AGE_MS = 30_000;
const observations = new Map();
const ruleLoaded = chrome.storage.local.get("repeatableUrl");
let queue = Promise.resolve();
let connected;

const hex = bytes => Array.from(new Uint8Array(bytes), n => n.toString(16).padStart(2, "0")).join("");
const utf8 = value => new TextEncoder().encode(value);
const sha = async value => hex(await crypto.subtle.digest("SHA-256", utf8(value)));

export function ordinaryUrl(raw) {
  try {
    const url = new URL(raw);
    if ((url.protocol !== "http:" && url.protocol !== "https:") || !url.hostname
        || url.username || url.password || url.search || url.hash || url.href.length > 4096) {
      return null;
    }
    return url.href;
  } catch {
    return null;
  }
}

function safeName(file) {
  const leaf = (file || "").split(/[\\/]/).pop()
    .replace(/[<>:"|?*\x00-\x1f]/g, "_").trim().replace(/^[.]+|[.]+$/g, "");
  if (!leaf || utf8(leaf).length > 180 || /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(leaf)) {
    return "download.bin";
  }
  return leaf;
}

export function qualifiedGet(item, configured, candidates, now) {
  if (!configured || ordinaryUrl(item.url) !== configured || item.state !== "in_progress"
      || item.paused || item.incognito || item.byExtensionId || ordinaryUrl(item.finalUrl) !== configured
      || (item.danger && item.danger !== "safe")) return null;
  const began = Date.parse(item.startTime);
  let referrerOrigin;
  try { referrerOrigin = new URL(item.referrer).origin; } catch { return null; }
  const possible = candidates.filter(entry =>
    entry.origin === referrerOrigin && Math.abs(entry.time - began) <= MAX_REQUEST_AGE_MS
    && now - entry.time < MAX_REQUEST_AGE_MS);
  if (possible.length !== 1 || possible[0].method !== "GET" || possible[0].private) return null;
  return possible[0];
}

async function recordObservation(details) {
  const url = ordinaryUrl(details.url);
  if (!url || details.tabId < 0 || !details.initiator || !details.requestHeaders) return;
  if ((await ruleLoaded).repeatableUrl !== url
      && (await chrome.storage.local.get("repeatableUrl")).repeatableUrl !== url) return;
  const now = Date.now();
  for (const [address, list] of observations) {
    const recent = list.filter(entry => now - entry.time < MAX_REQUEST_AGE_MS);
    if (recent.length) observations.set(address, recent);
    else observations.delete(address);
  }
  if (!observations.has(url) && observations.size >= 256) return;
  const entries = observations.get(url) || [];
  const entry = {
    time: details.timeStamp, origin: details.initiator, tab: details.tabId,
    frame: details.frameId, document: details.documentId,
    requestId: details.requestId, method: details.method,
    private: details.requestHeaders.some(({name, value}) =>
      /^(authorization|cookie|proxy-authorization)$/i.test(name) && (value || "").length),
  };
  // Any ambiguity is preserved as a blocker, including competing POSTs.
  const previous = entries.findIndex(value => value.requestId === entry.requestId);
  if (previous >= 0) {
    entry.private ||= entries[previous].private;
    entries[previous] = entry;
  } else if (entries.length < 16) {
    entries.push(entry);
  }
  observations.set(url, entries);
}

function envelope(op, key, payload) {
  return { version: 1, request_id: crypto.randomUUID(), idempotency_key: key, op, payload };
}

function createPort() {
  const port = chrome.runtime.connectNative(NATIVE_HOST);
  const waiting = new Map();
  let open = true;
  function close() {
    if (!open) return;
    open = false;
    for (const deliver of waiting.values()) deliver(null);
    waiting.clear();
    if (connected?.port === port) connected = undefined;
  }
  port.onDisconnect.addListener(() => { void chrome.runtime.lastError; close(); });
  port.onMessage.addListener(response => waiting.get(response.request_id)?.(response));
  return {
    port,
    call: req => new Promise(resolve => {
      if (!open) { resolve(null); return; }
      const timeout = setTimeout(() => { close(); port.disconnect(); }, 5000);
      waiting.set(req.request_id, reply => {
        clearTimeout(timeout);
        waiting.delete(req.request_id);
        resolve(reply);
      });
      try { port.postMessage(req); } catch { close(); }
    }),
  };
}

async function connect() {
  if (connected) return connected;
  const client = createPort();
  try {
    await chrome.storage.local.setAccessLevel({ accessLevel: "TRUSTED_CONTEXTS" });
    let { installationKey } = await chrome.storage.local.get("installationKey");
    if (!/^[0-9a-f]{64}$/.test(installationKey ?? "")) {
      installationKey = hex(crypto.getRandomValues(new Uint8Array(32)));
      await chrome.storage.local.set({ installationKey });
    }
    async function hello(stage, fields) {
      const result = await client.call(envelope("hello", crypto.randomUUID(), { stage, supported_versions: [1], ...fields }));
      if (result?.status !== "ok") throw new Error("native host unavailable");
      return result.data;
    }
    await hello("negotiate", {});
    const { registered_instance_id: instance } = await hello("enroll", { enroll_new_instance: true, installation_key: installationKey });
    const { challenge_id: challenge, challenge: nonce } =
      await hello("challenge", { registered_instance_id: instance });
    const origin = chrome.runtime.getURL("");
    if (!/^[0-9a-f]{64}$/.test(instance)) {
      throw new Error("unexpected native origin");
    }
    const key = await crypto.subtle.importKey("raw",
      Uint8Array.from(installationKey.match(/../g), pair => parseInt(pair, 16)),
      { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
    const rawProof = await crypto.subtle.sign("HMAC", key,
      utf8("rustcatch-hello-v1:" + challenge + ":" + nonce + ":" + origin + ":" + instance));
    await hello("prove", { registered_instance_id: instance, challenge_id: challenge, auth_proof: hex(rawProof) });
    connected = { ...client, instance };
    return connected;
  } catch (error) {
    client.port.disconnect();
    throw error;
  }
}

async function send(op, key, payload) {
  const c = await connect();
  return c.call(envelope(op, key, payload));
}

function output(fileName) {
  return { directory_policy: "category_default", file_name_hint: fileName };
}

function directSource(url, evidence) {
  return {
    kind: "direct_http", url, method: "GET", replayability: evidence ? "known_replayable" : "unknown",
    ...(evidence ? { replayability_evidence_ref: evidence } : {}),
  };
}

function report(message) {
  return chrome.storage.local.set({ bridgeStatus: message });
}

const outbox = async () => (await chrome.storage.local.get("handoffOutbox")).handoffOutbox ?? {};
async function keep(job, phase) {
  const jobs = await outbox();
  jobs[job.key] = { ...job, phase };
  await chrome.storage.local.set({ handoffOutbox: jobs });
}
async function remove(job) {
  const jobs = await outbox();
  delete jobs[job.key];
  await chrome.storage.local.set({ handoffOutbox: jobs });
}

async function lookup(job) {
  const [item] = await chrome.downloads.search({ id: job.id });
  return item?.startTime === job.start && ordinaryUrl(item.url) === job.url ? item : null;
}

export function cancellationConfirmed(item) {
  return item?.state === "interrupted" && item.error === "USER_CANCELED";
}

async function handoff(job) {
  let item = await lookup(job);
  if (!item) {
    await report("找不到原浏览器下载记录，保留交接状态以便核对。");
    return;
  }
  const client = await connect();
  const known = await send("handoff.status", job.key, {});
  if (known?.durable && ["committed", "aborted"].includes(known.data?.handoff_state)) {
    await remove(job);
    await report(known.data.handoff_state === "committed" ? "已交给 RustCatch。" : "桌面交接已中止。");
    return;
  }
  const prepare = await send("handoff.prepare", job.key, {
    mode: "browser_intercept",
    browser: {
      installation_id: client.instance, profile_id: client.instance, download_id: job.id,
      tab_id: job.tab, frame_id: job.frame, document_id: job.document, nav_epoch: 1,
    },
    source: directSource(job.url, "user-repeatable:" + await sha(job.url)),
    output: output(job.name), context_ref: null, network_profile_id: "system-default", media_intent: null,
  });
  if (prepare?.status !== "prepared" || prepare.durable !== true || !prepare.handoff_token) {
    await report("桌面未确认可接管。浏览器下载由原状态保留，请在桌面核对后重试。");
    return;
  }
  const binding = {
    task_id: prepare.task_id, expected_revision: prepare.revision, handoff_token: prepare.handoff_token,
  };
  item = await lookup(job);
  if (!item) {
    await report("无法核对原浏览器下载，桌面暂不启动。");
    return;
  }
  const configured = (await chrome.storage.local.get("repeatableUrl")).repeatableUrl;
  let cancelRequested = job.phase === "cancel_sent";
  if (job.phase === "preparing" && item.state === "in_progress" && !item.paused
      && ordinaryUrl(item.finalUrl) === job.url
      && configured === job.url && Date.parse(prepare.expires_at) > Date.now() + 10_000) {
    await keep(job, "cancel_sent");
    cancelRequested = true;
    try { await chrome.downloads.cancel(job.id); } catch { /* Browser status determines the next action. */ }
    item = await lookup(job);
  }
  if (cancelRequested && cancellationConfirmed(item)) {
    await keep(job, "cancel_sent");
    const committed = await send("handoff.commit", job.key, {
      ...binding, browser_download_id: job.id, browser_cancel_confirmed: true,
    });
    if (committed?.status === "queued" && committed.durable === true) {
      await remove(job);
      await report("已交给 RustCatch。");
      return;
    }
    await report("浏览器已取消，桌面尚未确认接管。保留记录，可按原任务重试。");
    return;
  }
  if (item?.state === "complete" || (!cancelRequested && cancellationConfirmed(item))
      || (job.phase === "preparing" && item?.state === "in_progress" && configured !== job.url)) {
    const aborted = await send("handoff.abort", job.key, binding);
    if (aborted?.status === "aborted" && aborted.durable === true) {
      await remove(job);
      await report(item.state === "complete" ? "浏览器已完成，桌面接管已中止。" : "已保留浏览器下载。");
      return;
    }
  }
  await report("原下载的取消状态尚未确认，桌面暂不启动。");
}

async function observeDownload(item) {
  const configured = (await chrome.storage.local.get("repeatableUrl")).repeatableUrl;
  const candidate = qualifiedGet(item, configured, observations.get(configured) ?? [], Date.now());
  if (!candidate) return;
  observations.delete(configured);
  if (Object.keys(await outbox()).length >= 32) {
    await report("待核对交接过多，新的下载交给浏览器。");
    return;
  }
  const job = {
    key: "download-" + crypto.randomUUID(), id: item.id, start: item.startTime, url: configured,
    name: safeName(item.filename || new URL(configured).pathname), tab: candidate.tab,
    frame: candidate.frame, document: candidate.document || "navigation-" + candidate.requestId,
    phase: "preparing",
  };
  await keep(job, job.phase);
  await handoff(job);
}

function serialize(action) {
  queue = queue.then(action).catch(() => report("连接桌面失败。浏览器下载保留，已记录的交接可以重试。"));
  return queue;
}

async function retry() {
  for (const job of Object.values(await outbox())) {
    await handoff(job);
  }
}

export async function queryCatCatchBridgeIfConfigured(extensionId, tabId) {
  if (!extensionId) return [];
  try {
    const response = await chrome.runtime.sendMessage(extensionId, { action: "getData", tabId });
    return Array.isArray(response) ? response : [];
  } catch {
    return [];
  }
}

chrome.webRequest.onBeforeSendHeaders.addListener(
  details => { void recordObservation(details).catch(() => {}); },
  { urls: ["http://*/*", "https://*/*"] }, ["requestHeaders", "extraHeaders"],
);
chrome.downloads.onCreated.addListener(item => { void serialize(() => observeDownload(item)); });
chrome.runtime.onStartup.addListener(() => { void serialize(retry); });
chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create({ id: "rustcatch-send-file", title: "发送公开文件链接到 RustCatch", contexts: ["link"] });
});
chrome.contextMenus.onClicked.addListener(({ menuItemId, linkUrl }) => {
  if (menuItemId !== "rustcatch-send-file") return;
  void serialize(async () => {
    const url = ordinaryUrl(linkUrl);
    if (!url) { await report("此链接含账号信息或查询参数，请使用浏览器下载。"); return; }
    const result = await send("task.add", "explicit-" + crypto.randomUUID(), {
      source: directSource(url, null), output: output(safeName(new URL(url).pathname)),
      network_profile_id: "system-default",
    });
    await report(result?.durable === true ? "公开链接已保存到桌面队列。" : "桌面下载器未接受此链接。");
  });
});
chrome.runtime.onMessage.addListener((message, sender, respond) => {
  if (sender.id !== chrome.runtime.id || sender.url !== chrome.runtime.getURL("popup.html")) return false;
  if (message?.type === "rustcatch.retry-handoff") {
    void serialize(retry).then(() => respond({ done: true }));
    return true;
  }
  return false;
});
