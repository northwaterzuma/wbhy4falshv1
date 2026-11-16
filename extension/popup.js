document.addEventListener("DOMContentLoaded", () => {
  const statusEl = document.getElementById("bridge-status");
  if (statusEl) {
    statusEl.textContent = "此版本尚未启用浏览器下载接管，请使用浏览器内置下载。";
  }
});
