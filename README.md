# sparktop ⚡️

top, but like with sparkles ✨

top can't answer "what caused everything to be slow like 30 seconds ago?" but
sparktop can!

![demo](sparktop.png)

## features

- [x] per-process cpu usage history
  - instead of just showing most recent sample, can show EWMA
  - can draw sparklines ▁▂▁▄▅▄
  - taller bars (`b`) give more vertical resolution; one full line = 100%, so
    multi-core (>100%) usage stacks visibly
- [x] sortable, toggleable columns
- [x] numeric columns heat-shaded (green→red) by value, so high values pop
- [x] hide idle (low-cpu) processes by default
- [x] process **detail view** (`⏎`): full-screen high-res braille charts of a
  process's cpu, memory and disk-i/o history

## usage

```bash
cargo run                  # 1s refresh, ewma weight 0.5
cargo run -- -d 0.5 -e 0.3 # custom refresh (s) and ewma weight (0..1)
./bin/local_deploy.sh      # install setuid-root to /usr/local/bin (needed to
                           # read other users' disk i/o on macOS)
```

### keys

| key      | action                                             |
|----------|----------------------------------------------------|
| `↑`/`↓`  | move selection                                     |
| `⏎`      | open detail view for the selected process          |
| `esc`    | back out of detail / a sub-mode                    |
| `s`      | choose sort column (repeat a column to reverse)    |
| `c`      | toggle which columns are shown                     |
| `i`      | show/hide idle (low-cpu) processes                 |
| `b`      | cycle bar height (1 → 2 → 3)                        |
| `q` / `^C` | quit                                             |

The footer always lists the keys for the current mode.

## todo

- display more stable process list
  - group stuff by sort key (perhaps deciles?) and then avoid resorting within each decile, since not that important strict ordering as much as rough neighborhood. use boxes or alternating background color
- list: toggle full/short process name
- list: other column options? state, ppid, etc.
- list: pid tree view
  - collapsable nodes, which aggregates values
  - key to fold all below certain depth
- list: regex folding
  - add regexs to create aggregation groups, expandable just like tree groups
  - name display is regex, maybe with representative match name?
- detail view: support multiple selected processes at once
- action: kill process
- process groups
  - regexs, subtrees, "selection" UI for arbitrary processes
  - can display all in detailed view, kill all, etc.
  - some notion of filtering/searching?

## inspo / places to steal from

- [bottom](https://github.com/ClementTsang/bottom/blob/309ebd8dc3ba35f80c93a296ebc688813e988d03/src/lib.rs#L348)
- [zenith](https://github.com/bvaisvil/zenith/blob/master/src/metrics.rs#L387)
- https://www.wezm.net/v2/posts/2020/rust-top-alternatives/
