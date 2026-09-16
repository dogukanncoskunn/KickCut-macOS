/*
 * Which hosts did the app actually talk to?
 *
 * Three captures taken while the UI test runs:
 *   dns.txt   - `tcpdump -n` of DNS: which names were looked up, and the
 *               addresses each lookup returned.
 *   conns.txt - `lsof -F pcn` of every process, polled: BSD sockets, which is
 *               what the Rust side (reqwest) uses.
 *   flows.txt - `nettop -L 0`: the kernel's flow table per process, which also
 *               covers Network.framework - what WebKit uses, invisible to lsof.
 *
 * DNS alone would include everything the runner itself does; the socket lists
 * alone give bare addresses. Joined, every connection is named by the lookup
 * that produced its address.
 *
 * The run fails when the app or its web view reaches a host outside ALLOWED,
 * or an address no lookup explains, or when nothing from the web view was seen
 * at all (which would mean the capture is blind, not that the app is quiet).
 *
 * Apple services that WebKit starts - Safe Browsing and its privacy lists - are
 * listed separately. They are macOS's traffic, not the app's, and carry nothing
 * about what the app does; they are reported so that claim is checked, not
 * assumed.
 *
 * Usage: node network-report.mjs dns.txt conns.txt flows.txt
 */
import { readFileSync } from "node:fs";

const ALLOWED = [
  /^kick\.com$/,
  /^[a-z0-9-]+\.kick\.com$/,
  /^ffmpeg\.martin-riedl\.de$/,
  /^github\.com$/,
  /^[a-z0-9-]+\.githubusercontent\.com$/,
];

// nettop cuts process names at 15 characters, lsof +c 0 does not.
const APP = /^(kickcut|com\.apple\.WebKi.*|ffmpeg|ffprobe)$/;
const WEBVIEW = /^com\.apple\.WebKi/;
const SYSTEM = /^(com\.apple\.Safar.*|webprivacyd)$/;

const [dnsFile, connsFile, flowsFile] = process.argv.slice(2);
const read = (f) => readFileSync(f, "utf8").split("\n");

const pending = new Map(); // "id port" -> queried name
const nameOf = new Map(); // address -> queried name
const aliasOf = new Map(); // CNAME target -> the name it stands in for
const rootOf = (name) => (aliasOf.has(name) ? rootOf(aliasOf.get(name)) : name);
for (const line of read(dnsFile)) {
  const q = line.match(/\.(\d+) > \S+\.53: (\d+)\+? .*?\b(?:A|AAAA|HTTPS|Type65)\? (\S+?)\.? \(/);
  if (q) {
    pending.set(`${q[2]} ${q[1]}`, q[3].toLowerCase());
    continue;
  }
  const r = line.match(/\.53 > \S+\.(\d+): (\d+)\S* (?:\S+ )*?\d+\/\d+\/\d+ (.*)$/);
  const asked = r && pending.get(`${r[2]} ${r[1]}`);
  if (!asked) continue;
  // images.kick.com is a CNAME for a CloudFront name, and macOS also looks that
  // target up on its own. Follow aliases back to the name the app asked for, or
  // the second lookup would relabel the same addresses as a stranger.
  const name = rootOf(asked);
  for (const m of r[3].matchAll(/\bCNAME (\S+?)\.?(?=,|\s|$)/g)) {
    if (m[1].toLowerCase() !== name) aliasOf.set(m[1].toLowerCase(), name);
  }
  for (const m of r[3].matchAll(/\b(?:A|AAAA) ([0-9a-f:.]+)/g)) nameOf.set(m[1].toLowerCase(), name);
}

const seen = new Map(); // "process\taddress" -> true
function note(process, addr) {
  addr = addr.toLowerCase();
  if (addr === "127.0.0.1" || addr === "::1" || addr.startsWith("*")) return;
  if (APP.test(process) || SYSTEM.test(process)) seen.set(`${process}\t${addr}`, true);
}

// lsof -F: "p<pid>", "c<command>", then "n<local>-><remote>" per socket.
let command = "?";
for (const line of read(connsFile)) {
  if (line.startsWith("c")) command = line.slice(1);
  if (!line.startsWith("n") || !line.includes("->")) continue;
  const remote = line.slice(line.indexOf("->") + 2);
  note(command, remote.startsWith("[") ? remote.replace(/^\[(.*)\]:\d+$/, "$1") : remote.replace(/:\d+$/, ""));
}

// nettop CSV: "time,name.pid,..." for a process, then "time,tcp4 local<->remote,..."
// for each of its flows. IPv4 ports follow a colon, IPv6 ports a dot.
command = "?";
for (const line of read(flowsFile)) {
  const field = line.split(",")[1] ?? "";
  const flow = field.match(/^(?:tcp|udp|quic)\S*\s+\S+<->(\S+)$/);
  if (!flow) {
    const proc = field.match(/^(.+)\.\d+$/);
    if (proc) command = proc[1];
    continue;
  }
  const remote = flow[1].replace(/%[^.:]+/, "");
  note(command, /^\d+\.\d+\.\d+\.\d+[:.]\d+$/.test(remote) ? remote.replace(/[:.]\d+$/, "") : remote.replace(/\.\d+$/, ""));
}

const app = new Map(); // host -> Set of processes
const system = new Map();
const problems = [];
for (const key of seen.keys()) {
  const [process, addr] = key.split("\t");
  const name = nameOf.get(addr);
  const isApp = APP.test(process);
  if (!name) {
    if (isApp) problems.push(`${addr} (${process}) was not produced by any lookup`);
    continue;
  }
  const table = isApp ? app : system;
  if (!table.has(name)) table.set(name, new Set());
  table.get(name).add(process);
  if (isApp && !ALLOWED.some((re) => re.test(name))) problems.push(`${name} (${process}) is not an expected host`);
}

const print = (table) => {
  console.log("| Host | Process |\n|---|---|");
  for (const [name, procs] of [...table].sort()) console.log(`| \`${name}\` | ${[...procs].join(", ")} |`);
};
console.log("### The app and its web view");
print(app);
console.log("\n### macOS services started by WebKit (not the app's traffic)");
print(system);

if (![...seen.keys()].some((k) => WEBVIEW.test(k))) {
  problems.push("no web view connections were captured - the capture is blind, not the app quiet");
}
if (problems.length) {
  console.log("\n**Problems:**\n" + problems.map((p) => `- ${p}`).join("\n"));
  process.exit(1);
}
