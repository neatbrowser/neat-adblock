# Engine parity: adblock-rust against uBlock Origin

Feeds the **same lists** and the **same requests** to NEAT's engine (adblock-rust, through
`nf-tool verdicts`) and to uBlock Origin's own static network filtering engine
([`@gorhill/ubo-core`](https://www.npmjs.com/package/@gorhill/ubo-core)), and counts the requests
the two judge differently. It measures the engine, not the lists: a request both stop is not
interesting, one that only one of them stops points at a rule the other does not understand.

    cd parity && npm install
    node gen-corpus.mjs ../dist corpus.jsonl [--rules 12000] [--log file.tsv ...]
    ../tools/nf-tool/target/release/nf-tool verdicts ../dist corpus.jsonl > rust.jsonl
    node run-ubo.mjs ../dist corpus.jsonl > ubo.jsonl
    node compare.mjs corpus.jsonl rust.jsonl ubo.jsonl > report.json

The corpus is made from the lists themselves (a URL that a sampled rule should match, in three
variants: the type and site the rule names, another request type, the same site as the host), plus
real requests from NEAT's `ads-requests.tsv` when given. "Stopped" means blocked or redirected.

The update workflow runs this on every changed release and writes the result to the run summary
(`parity-report.json` in the `bench-report` artifact).
