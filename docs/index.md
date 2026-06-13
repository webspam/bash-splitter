---
# https://vitepress.dev/reference/default-theme-home-page
layout: home

hero:
  name: "bash-splitter"
  text: "Split a bash command into its individual commands"
  tagline: Inspect each command on its own, for example against allow/deny rules.
  actions:
    - theme: brand
      text: Guide
      link: /guide/
    - theme: alt
      text: Reference
      link: /reference/coverage
    - theme: alt
      text: GitHub
      link: https://github.com/webspam/bash-splitter

features:
  - title: Real bash parsing
    details: Built on the brush parser, not string matching, so splitting holds even when constructs nest.
    link: /reference/coverage
  - title: Flat & nested modes
    details: List every command at the top level for filtering, or keep the substitution hierarchy with -n.
    link: /guide/#two-modes
  - title: Rich per-stage metadata
    details: Redirects, loop detection, and the variables each stage expands, captured per command.
    link: /reference/redirects
---
