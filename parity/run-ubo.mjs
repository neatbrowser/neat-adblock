// usage: node run-ubo.mjs <dist> <requests.jsonl>  > verdicts.jsonl
import { StaticNetFilteringEngine } from '@gorhill/ubo-core';
import { networkLists, readJsonl } from './lib.mjs';

const [dist, requests] = process.argv.slice(2);
const snfe = await StaticNetFilteringEngine.create();
await snfe.useLists(networkLists(dist).map(l => ({ name: l.name, raw: l.text })));

const type = t => (t === 'subdocument' ? 'sub_frame' : t);
const out = [];
for (const r of readJsonl(requests)) {
  const res = snfe.matchRequest({ originURL: r.source, url: r.url, type: type(r.type) });
  const log = res !== 0 ? snfe.toLogData() : null;
  const raw = log?.raw ?? null;
  out.push(JSON.stringify({
    block: res === 1,
    redirect: res === 1 && /[$,](redirect|rewrite)=/.test(raw ?? ''),
    exception: res === 2,
    rule: res === 1 ? raw : null,
    exceptionRule: res === 2 ? raw : null,
  }));
}
process.stdout.write(out.join('\n') + '\n');
