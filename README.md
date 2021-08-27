# sparktop ⚡️

top, but like with sparkles ✨

top can't answer "what caused everything to be slow like 30 seconds ago?" but
sparktop can!

![demo](sparktop.png)

## features

- [x] per-process cpu usage history
  - instead of just showing most recent sample, can show EWMA
  - can draw sparklines ▁▂▁▄▅▄

## todo

- display more stable process list
  - group stuff by sort key (perhaps deciles?) and then avoid resorting within each decile, since not that important strict ordering as much as rough neighborhood. use boxes or alternating background color
- list: toggle columns
- list: toggle other sparkline graphs
- list: toggle full/short process name
- list: other column options? state, ppid, etc.
- list: pid tree view
  - collapsable nodes, which aggregates values
  - key to fold all below certain depth
- list: regex folding
  - add regexs to create aggregation groups, expandable just like tree groups
  - name display is regex, maybe with representative match name?
- detailed view: for all selected processes, show higher-granularity sparkline
- action: kill process
- process groups
  - regexs, subtrees, "selection" UI for arbitrary processes
  - can display all in detailed view, kill all, etc.
  - some notion of filtering/searching?

## inspo / places to steal from

- [bottom](https://github.com/ClementTsang/bottom/blob/309ebd8dc3ba35f80c93a296ebc688813e988d03/src/lib.rs#L348)
- [zenith](https://github.com/bvaisvil/zenith/blob/master/src/metrics.rs#L387)
- https://www.wezm.net/v2/posts/2020/rust-top-alternatives/
