const form = document.getElementById("rule");
const input = document.getElementById("url");
const confirmation = document.getElementById("confirmation");
const status = document.getElementById("bridge-status");

async function refresh() {
  const { repeatableUrl, bridgeStatus } = await chrome.storage.local.get(["repeatableUrl", "bridgeStatus"]);
  input.value = repeatableUrl || "";
  confirmation.checked = Boolean(repeatableUrl);
  status.textContent = bridgeStatus || "桌面运行时，可以接管已授权的公开文件链接。";
}

form.addEventListener("submit", async event => {
  event.preventDefault();
  try {
    const url = new URL(input.value.trim());
    if (!confirmation.checked || !["https:", "http:"].includes(url.protocol) || !url.hostname
        || url.username || url.password || url.search || url.hash || url.href.length > 4096) {
      throw new Error("请确认并输入无需登录、可重复下载且无参数的完整 HTTP/HTTPS 链接。");
    }
    const allowed = await chrome.permissions.request({ origins: [url.protocol + "//" + url.hostname + "/*"] });
    if (!allowed) throw new Error("未获得站点权限，浏览器将继续下载。");
    const { repeatableUrl: previous } = await chrome.storage.local.get("repeatableUrl");
    if (previous) {
      const old = new URL(previous);
      if (old.origin !== url.origin && (old.protocol !== url.protocol || old.hostname !== url.hostname)) {
        await chrome.permissions.remove({ origins: [old.protocol + "//" + old.hostname + "/*"] });
      }
    }
    await chrome.storage.local.set({ repeatableUrl: url.href, bridgeStatus: "已启用此链接的接管规则。" });
    await refresh();
  } catch (error) {
    status.textContent = error.message;
  }
});

document.getElementById("disable").addEventListener("click", async () => {
  const { repeatableUrl } = await chrome.storage.local.get("repeatableUrl");
  await chrome.storage.local.remove("repeatableUrl");
  if (repeatableUrl) {
    const url = new URL(repeatableUrl);
    await chrome.permissions.remove({ origins: [url.protocol + "//" + url.hostname + "/*"] });
  }
  await chrome.storage.local.set({ bridgeStatus: "已关闭自动接管。" });
  await refresh();
});

document.getElementById("retry").addEventListener("click", async () => {
  status.textContent = "正在核对原下载状态…";
  try {
    await chrome.runtime.sendMessage({ type: "rustcatch.retry-handoff" });
    await refresh();
  } catch {
    status.textContent = "桌面连接不可用，请打开 RustCatch 后重试。";
  }
});

const urlChanged = (changes, area) => {
  if (area === "local" && changes.bridgeStatus) status.textContent = changes.bridgeStatus.newValue;
};
chrome.storage.onChanged.addListener(urlChanged);
void refresh();
