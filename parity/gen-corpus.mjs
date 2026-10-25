// usage: node gen-corpus.mjs <dist> <out.jsonl> [--rules N] [--seed N] [--log ads-requests.tsv ...]
import fs from 'node:fs';
import { networkLists, rng } from './lib.mjs';

const args = process.argv.slice(2);
const dist = args[0];
const out = args[1];
let maxRules = 12000;
let seed = 1;
const logs = [];
for (let i = 2; i < args.length; i++) {
  if (args[i] === '--rules') maxRules = Number(args[++i]);
  else if (args[i] === '--seed') seed = Number(args[++i]);
  else if (args[i] === '--log') logs.push(args[++i]);
}

const TYPES = new Map([
  ['script', 'script'], ['image', 'image'], ['stylesheet', 'stylesheet'], ['css', 'stylesheet'],
  ['xmlhttprequest', 'xmlhttprequest'], ['xhr', 'xmlhttprequest'], ['subdocument', 'subdocument'],
  ['frame', 'subdocument'], ['media', 'media'], ['font', 'font'], ['websocket', 'websocket'],
  ['ping', 'ping'], ['other', 'other'], ['object', 'other'],
]);
const ALL_TYPES = ['script', 'image', 'stylesheet', 'xmlhttprequest', 'subdocument', 'media', 'font', 'ping', 'other'];
const HOST = /^[a-z0-9]([a-z0-9-]*[a-z0-9])?(\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)+$/;

/** One filter line -> { url, types, sources, ... } or null when no URL can be made from it. */
function fromRule(line) {
  if (/#[@?$%]?#|#\?#|\+js\(/.test(line) && !line.includes('$')) return null;
  if (line.includes('##') || line.includes('#@#') || line.includes('#?#') || line.includes('#$#')) return null;
  let rest = line;
  const exception = rest.startsWith('@@');
  if (exception) rest = rest.slice(2);

  let pattern = rest;
  let options = '';
  const dollar = rest.lastIndexOf('$');
  if (dollar > 0 && !/^\/.*\/$/.test(rest.slice(0, dollar))) {
    pattern = rest.slice(0, dollar);
    options = rest.slice(dollar + 1);
  }
  if (pattern.length >= 2 && pattern.startsWith('/') && pattern.endsWith('/')) return null;   // regex

  const types = [];
  const notTypes = [];
  let party = null;
  let domains = [];
  for (const o of options.split(',')) {
    const neg = o.startsWith('~');
    const name = (neg ? o.slice(1) : o).split('=')[0];
    if (TYPES.has(name)) (neg ? notTypes : types).push(TYPES.get(name));
    else if (name === 'third-party' || name === '3p') party = neg ? '1p' : '3p';
    else if (name === 'first-party' || name === '1p') party = neg ? '3p' : '1p';
    else if (name === 'domain' || name === 'from') domains = o.slice(o.indexOf('=') + 1).split('|');
    else if (['popup', 'document', 'doc', 'all', 'csp', 'removeparam', 'redirect-rule', 'replace', 'header',
              'ipaddress', 'cname', 'permissions', 'inline-script', 'inline-font', 'generichide', 'elemhide',
              'ghide', 'specifichide', 'shide', 'ehide', 'to', 'denyallow', 'method', 'match-case', 'urlskip',
              'redirect', 'rewrite', 'empty', 'mp4', 'badfilter', 'important', 'strict1p', 'strict3p',
              'app', 'reason', 'uritransform', 'requestheader', 'responseheader'].includes(name)) {
      if (['popup', 'document', 'doc', 'all', 'csp', 'removeparam', 'replace', 'header', 'ipaddress', 'cname',
           'permissions', 'inline-script', 'inline-font', 'generichide', 'elemhide', 'ghide', 'specifichide',
           'shide', 'ehide', 'to', 'denyallow', 'method', 'match-case', 'urlskip', 'rewrite', 'empty', 'mp4',
           'badfilter', 'app', 'reason', 'uritransform', 'requestheader', 'responseheader'].includes(name)) return null;
    }
  }
  const dom = domains.filter(d => d && !d.startsWith('~') && !d.includes('/') && !d.includes('*'));
  if (domains.length && !dom.length) domains = ['~'];     // only negated domains: any other site

  let url;
  const p = pattern.replace(/\|$/, '');
  if (p.startsWith('||')) {
    const m = /^\|\|([^\^\/*:?|]+)(.*)$/.exec(p);
    if (!m || !HOST.test(m[1])) return null;
    const tail = m[2].replace(/\^/g, '/').replace(/\*/g, 'x');
    url = 'https://' + m[1] + (tail.startsWith('/') || tail === '' ? tail || '/' : '/' + tail);
    return { url, host: m[1], types, notTypes, party, domain: dom[0] ?? null, exception };
  }
  if (/^\|https?:\/\//.test(p)) {
    url = p.slice(1).replace(/\^/g, '/').replace(/\*/g, 'x');
  } else if (p) {
    const tail = p.replace(/^\|/, '').replace(/\^/g, '/').replace(/\*/g, 'x');
    url = 'https://cdn.example.net/' + tail.replace(/^\//, '');
  } else {
    return null;
  }
  try {
    const u = new URL(url);
    return { url: u.href, host: u.hostname, types, notTypes, party, domain: dom[0] ?? null, exception };
  } catch (e) {
    return null;
  }
}

const rand = rng(seed);
const candidates = [];
for (const { text } of networkLists(dist)) {
  for (const line of text.split('\n')) {
    if (!line || line[0] === '!') continue;
    if (rand() < 1) candidates.push(line);
  }
}

// Uniform sample of the rules (a seeded shuffle of the first part is enough).
const picked = [];
const want = Math.min(maxRules, candidates.length);
const seen = new Set();
while (picked.length < want && seen.size < candidates.length) {
  const i = Math.floor(rand() * candidates.length);
  if (seen.has(i)) continue;
  seen.add(i);
  const r = fromRule(candidates[i]);
  if (r) picked.push({ ...r, rule: candidates[i] });
}

const lines = new Set();
const add = (url, source, type, why) => lines.add(JSON.stringify({ url, source, type, why }));
for (const r of picked) {
  const allowedTypes = r.types.length ? r.types : ALL_TYPES.filter(t => !r.notTypes.includes(t));
  const t1 = allowedTypes[0] ?? 'script';
  const site = r.domain ? `https://${r.domain}/` : r.party === '1p' ? `https://${r.host}/` : 'https://www.example.org/';
  add(r.url, site, t1, r.rule);
  const other = ALL_TYPES.find(t => !allowedTypes.includes(t)) ?? 'image';
  add(r.url, site, other, r.rule + '  [another type]');
  add(r.url, `https://${r.host}/`, t1, r.rule + '  [same site]');
  add(r.url, 'https://www.example.org/', t1, r.rule + '  [unrelated site]');
}

// Real requests of NEAT.
const logType = { Fetch: 'xmlhttprequest', XmlHttpRequest: 'xmlhttprequest', Script: 'script', Image: 'image',
  Stylesheet: 'stylesheet', Font: 'font', Media: 'media', Ping: 'ping', CspViolationReport: 'ping', Document: 'subdocument' };
let real = 0;
for (const f of logs) {
  for (const l of fs.readFileSync(f, 'utf8').split('\n')) {
    const c = l.split('\t');
    if (c.length < 6 || l.startsWith('#') || c[1].startsWith('skip')) continue;
    if (!/^https?:/.test(c[5]) || !c[4]) continue;
    add(c[5], `https://${c[4]}/`, logType[c[2]] ?? 'other', 'real request');
    real++;
  }
}

fs.writeFileSync(out, [...lines].join('\n') + '\n');
console.error(`${picked.length} rules sampled, ${real} real requests, ${lines.size} requests written`);
