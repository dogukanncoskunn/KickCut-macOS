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
  // The tour after a reload keeps the Turkish it asked for.
  if (!sessionStorage.getItem("kickcut.e2e.tour")) localStorage.setItem("kickcut.locale", "en");
  localStorage.setItem("kickcut.outputDir", OUT_DIR);

  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  // Long enough for the runner's 2 s screenshots to catch every screen.
  const hold = () => sleep(5000);
  const TOUR = "kickcut.e2e.tour";

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
    await hold();
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
        const select = await until("a broadcast", () => button("Select"), 30_000);
        await hold();
        select.click();
        sessionStorage.setItem(TOUR + ".channel", channel);
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
    await hold();

    say("adding to the queue");
    (await until("an enabled Add to queue", () => button("Add to queue"), 60_000)).click();
    await sleep(20_000); // long enough to photograph the rail while it downloads

    say("waiting for the download");
    button("Downloads").click();
    await until("a completed job", () => document.body.textContent.includes("Completed "), 600_000);
    await hold();

    // The same screens in Turkish and the light theme. Both are read from
    // localStorage on the first paint, so they take a reload. The Finder
    // reveal comes last, since its window covers the app from then on.
    say("switching to Turkish, light theme");
    sessionStorage.setItem(TOUR, "1");
    localStorage.setItem("kickcut.locale", "tr");
    localStorage.setItem("kickcut.theme", "light");
    location.reload();
  }

  async function tour() {
    sessionStorage.removeItem(TOUR);
    const tab = (i) => document.querySelectorAll("nav button")[i].click();

    say("tour: Yayınlar");
    // Disabled until the field has text, so not found through button().
    const submit = await until("the channel form", () =>
      buttons().find((b) => b.textContent.trim().startsWith("Yayınları listele")), 30_000);
    type(submit.closest("form").querySelector("input"), sessionStorage.getItem(TOUR + ".channel") || CHANNELS[0]);
    await sleep(300);
    submit.click();
    const select = await until("a broadcast", () => button("Seç"), 30_000);
    await hold();

    say("tour: İndirme");
    select.click();
    await until("the range fields", () => document.querySelector("input[inputmode=numeric]"), 60_000);
    await hold();

    say("tour: İndirilenler");
    tab(2);
    await hold();

    say("tour: Ayarlar");
    tab(3);
    await hold();

    say("revealing in Finder");
    tab(2);
    (await until("Klasörde göster", () => button("Klasörde göster"), 10_000)).click();
    await hold();
    say("finished");
  }

  window.addEventListener("load", () => {
    (sessionStorage.getItem(TOUR) ? tour() : run()).catch((e) => say("FAILED: " + e.message, true));
  });
})();
