/*
 * Drives the real, release-built app through the one flow a user cares about:
 * install FFmpeg, find a broadcast, cut 30 seconds of it, reveal the result.
 *
 * Injected into index.html by ui-e2e.yml only - it never ships. It runs inside
 * the app's own WKWebView, because driving the window from outside needs
 * Accessibility permission the runner does not grant. Everything below goes
 * through the UI (clicks, typing, React's own handlers); the only direct IPC
 * call is polling ffmpeg_status to know when the install has finished.
 *
 * The workflow decides pass/fail from the MP4 on disk, not from this script.
 * What this script adds is the banner, so every screenshot says which step the
 * run was on - and a red one when a step gave up.
 */
(() => {
  const CHANNELS = "__CHANNELS__".split(",");
  const OUT_DIR = "__OUT_DIR__";

  // Before React mounts: English labels to find buttons by, and a folder so the
  // native picker - which a script cannot click - is never needed.
  localStorage.setItem("kickcut.locale", "en");
  localStorage.setItem("kickcut.outputDir", OUT_DIR);

  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

  let banner;
  function say(text, failed = false) {
    if (!banner) {
      banner = document.createElement("div");
      banner.style.cssText =
        "position:fixed;left:12px;bottom:12px;z-index:99999;padding:6px 10px;border-radius:6px;" +
        "font:600 12px ui-monospace,monospace;color:#fff;pointer-events:none";
      document.body.appendChild(banner);
    }
    banner.style.background = failed ? "#c0262d" : "#1f7a3a";
    banner.textContent = "E2E · " + text;
  }

  async function until(what, find, timeoutMs) {
    const end = Date.now() + timeoutMs;
    while (Date.now() < end) {
      const found = await find();
      if (found) return found;
      await sleep(500);
    }
    throw new Error("timed out waiting for " + what);
  }

  const buttons = () => [...document.querySelectorAll("button")];
  const button = (prefix) =>
    buttons().find((b) => b.textContent.trim().startsWith(prefix) && !b.disabled);

  function type(input, value) {
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;
    input.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    setter.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  }
  const blur = (input) => input.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));

  async function timeBox(label, digits) {
    const box = [...document.querySelectorAll("label")].find(
      (l) => l.querySelector("span")?.textContent.trim() === label,
    );
    const input = box?.querySelector("input");
    if (!input) throw new Error("no " + label + " field");
    type(input, digits);
    await sleep(400); // let React render the typed text before it is committed
    blur(input);
    await sleep(800);
  }

  async function run() {
    say("installing FFmpeg");
    (await until("the Settings tab", () => button("Settings"), 30_000)).click();
    (await until("the FFmpeg button", () => button("Install FFmpeg") || button("Reinstall"), 30_000)).click();
    await until(
      "a managed FFmpeg",
      async () => (await window.__TAURI_INTERNALS__.invoke("ffmpeg_status")).source === "managed",
      300_000,
    );

    let picked = false;
    for (const channel of CHANNELS) {
      say("listing " + channel);
      button("Broadcasts").click();
      const submit = await until("the channel form", () =>
        buttons().find((b) => b.textContent.trim().startsWith("List broadcasts")), 20_000);
      type(submit.closest("form").querySelector("input"), channel);
      await sleep(300);
      submit.click();
      try {
        (await until("a broadcast", () => button("Select"), 30_000)).click();
        picked = true;
        break;
      } catch {
        // No VODs, or an unknown channel; try the next one.
      }
    }
    if (!picked) throw new Error("none of " + CHANNELS.join(", ") + " listed a broadcast");

    say("setting a 30 s range");
    await until("the range fields", () => document.querySelector("input[inputmode=numeric]"), 60_000);
    await sleep(1500);
    await timeBox("Start", "000000"); // start first, or the end is clamped to it
    await timeBox("End", "000030");

    say("adding to the queue");
    (await until("an enabled Add to queue", () => button("Add to queue"), 60_000)).click();
    await sleep(20_000); // long enough to photograph the rail while it downloads

    say("waiting for the download");
    button("Downloads").click();
    await until("a completed job", () => document.body.textContent.includes("Completed "), 600_000);

    say("revealing in Finder");
    (await until("Show in folder", () => button("Show in folder"), 10_000)).click();
    await sleep(1000);
    say("finished");
  }

  window.addEventListener("load", () => {
    run().catch((e) => say("FAILED: " + e.message, true));
  });
})();
