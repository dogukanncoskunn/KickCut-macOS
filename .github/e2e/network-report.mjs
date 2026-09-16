/*
 * Which hosts did the app actually talk to?
 *
 * Two captures taken while the UI test runs:
 *   dns.txt   - `tcpdump -n` of DNS: which names were looked up, and the
 *               addresses each lookup returned.
 *   conns.txt - `lsof -F pcn` polled continuously for the app, WebKit's
 *               networking process and FFmpeg: which addresses those processes
 *               held connections to.
 *
 * DNS alone would include everything the runner itself does; lsof alone gives
 * bare addresses. Joined, every connection the app made is named by the lookup
 * that produced its address. A connection whose address no lookup explains, or
 * a name outside the list below, fails the run.
 *
 * Usage: node network-report.mjs dns.txt conns.txt
 */
import { readFileSync } from "node:fs";

const ALLOWED = [
  /^kick\.com$/,
  /^[a-z0-9-]+\.kick\.com$/,
  /^ffmpeg\.martin-riedl\.de$/,
  /^github\.com$/,
  /^[a-z0-9-]+\.githubusercontent\.com$/,
];

const [dnsFile, connsFile] = process.argv.slice(2);
const pending = new Map(); // "id port" -> queried name
const nameOf = new Map(); // address -> queried name

for (const line of readFileSync(dnsFile, "utf8").split("\n")) {
  const q = line.match(/\.(\d+) > \S+\.53: (\d+)\+? .*?\b(?:A|AAAA|HTTPS|Type65)\? (\S+?)\.? \(/);
  if (q) {
    pending.set(`${q[2]} ${q[1]}`, q[3].toLowerCase());
    continue;
  }
  const r = line.match(/\.53 > \S+\.(\d+): (\d+)\S* (?:\S+ )*?\d+\/\d+\/\d+ (.*)$/);
  if (!r) continue;
  const name = pending.get(`${r[2]} ${r[1]}`);
  if (!name) continue;
  for (const m of r[3].matchAll(/\b(?:A|AAAA) ([0-9a-f:.]+)/g)) nameOf.set(m[1].toLowerCase(), name);
}

// lsof covers every process; these are the ones that act for the app.
const OURS = /^(kickcut|com\.apple\.WebKit\.Networking|ffmpeg|ffprobe)$/;

const seen = new Map(); // remote address -> Set of process names
let command = "?";
for (const line of readFileSync(connsFile, "utf8").split("\n")) {
  if (line.startsWith("c")) command = line.slice(1);
  if (!line.startsWith("n") || !line.includes("->") || !OURS.test(command)) continue;
  const remote = line.slice(line.indexOf("->") + 2);
  const addr = (
    remote.startsWith("[") ? remote.replace(/^\[(.*)\]:\d+$/, "$1") : remote.replace(/:\d+$/, "")
  ).toLowerCase();
  if (!seen.has(addr)) seen.set(addr, new Set());
  seen.get(addr).add(command);
}

const hosts = new Map(); // name -> { addresses, processes }
const problems = [];
for (const [addr, procs] of seen) {
  if (addr === "127.0.0.1" || addr === "::1") continue;
  const name = nameOf.get(addr);
  if (!name) {
    problems.push(`${addr} (${[...procs].join(", ")}) was not produced by any lookup`);
    continue;
  }
  const entry = hosts.get(name) ?? { addresses: new Set(), processes: new Set() };
  entry.addresses.add(addr);
  procs.forEach((p) => entry.processes.add(p));
  hosts.set(name, entry);
  if (!ALLOWED.some((re) => re.test(name))) problems.push(`${name} is not an expected host`);
}

console.log("| Host | Connections | Process |\n|---|---|---|");
for (const [name, e] of [...hosts].sort()) {
  console.log(`| \`${name}\` | ${e.addresses.size} | ${[...e.processes].join(", ")} |`);
}
if (seen.size === 0) problems.push("no connections were captured at all - the capture is broken");
if (problems.length) {
  console.log("\n**Problems:**\n" + problems.map((p) => `- ${p}`).join("\n"));
  process.exit(1);
}
