// usage: node compare.mjs <requests.jsonl> <rust.jsonl> <ubo.jsonl>  > report.json
import { readJsonl } from './lib.mjs';

const [rf, af, uf] = process.argv.slice(2);
const reqs = readJsonl(rf);
const rust = readJsonl(af);
const ubo = readJsonl(uf);
if (reqs.length !== rust.length || reqs.length !== ubo.length)
  throw new Error(`line counts differ: ${reqs.length} requests, ${rust.length} rust, ${ubo.length} ubo`);

const stopped = v => v.block || v.redirect;
let agree = 0;
const onlyRust = [];
const onlyUbo = [];
for (let i = 0; i < reqs.length; i++) {
  const a = stopped(rust[i]);
  const u = stopped(ubo[i]);
  if (a === u) { agree++; continue; }
  (a ? onlyRust : onlyUbo).push({ ...reqs[i], rust: rust[i], ubo: ubo[i] });
}

// Group by the rule that decided, so a few rules do not look like thousands of requests.
function group(list, side) {
  const by = new Map();
  for (const m of list) {
    const key = side === 'rust' ? (m.rust.rule ?? 'exception: ' + m.ubo.exceptionRule) : (m.ubo.rule ?? 'exception: ' + m.rust.exceptionRule);
    const g = by.get(key) ?? { rule: key, count: 0, sample: m };
    g.count++;
    by.set(key, g);
  }
  return [...by.values()].sort((x, y) => y.count - x.count).slice(0, 40).map(g => ({
    rule: g.rule.slice(0, 160), count: g.count, url: g.sample.url, source: g.sample.source, type: g.sample.type,
    rust: g.sample.rust.rule ?? g.sample.rust.exceptionRule, ubo: g.sample.ubo.rule ?? g.sample.ubo.exceptionRule,
  }));
}

const total = reqs.length;
console.log(JSON.stringify({
  total,
  agree,
  agreePercent: Math.round((agree / total) * 10000) / 100,
  stoppedByBoth: reqs.filter((_, i) => stopped(rust[i]) && stopped(ubo[i])).length,
  onlyRustStops: onlyRust.length,
  onlyUboStops: onlyUbo.length,
  realRequests: reqs.filter(r => r.why === 'real request').length,
  realDisagree: [...onlyRust, ...onlyUbo].filter(m => m.why === 'real request').length,
  onlyUboStopsByRule: group(onlyUbo, 'ubo'),
  onlyRustStopsByRule: group(onlyRust, 'rust'),
}, null, 2));
