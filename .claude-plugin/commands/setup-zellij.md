---
description: Print the Zellij config snippet required to load the zellij-smart-tabs plugin
---

Print the config snippet the user should add to `~/.config/zellij/config.kdl`, using the current released WASM URL:

```kdl
plugins {
    smart-tabs location="https://github.com/YesYouKenSpace/zellij-smart-tabs/releases/latest/download/zellij-smart-tabs.wasm" {}
}

load_plugins {
    smart-tabs
}
```

After pasting and saving, instruct the user to restart Zellij:

```bash
zellij kill-all-sessions -y && zellij
```

Tell the user this is a one-time setup per machine.

Do not edit the file automatically — print the snippet and instructions for the user to paste in manually. Config changes to dotfiles should always be under user control.
