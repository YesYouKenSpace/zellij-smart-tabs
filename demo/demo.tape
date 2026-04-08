# zellij-smart-tabs demo
# Run: vhs demo.tape
# Requires: vhs, zellij, zellij-smart-tabs installed

Output demo.webm

Require zellij

Set Shell "zsh"
Set FontSize 16
Set FontFamily "FiraCode Nerd Font"
Set Width 1400
Set Height 700
Set Padding 10
Set TypingSpeed 30ms
Set Theme "Catppuccin Mocha"

# Start Zellij with 3 tabs in different directories
Type "zellij --config demo-config.kdl -n demo-layout.kdl"
Enter
Sleep 1s

# --- Scene 1: Tabs auto-renamed from the start ---
Type "echo '📌 Tabs are auto-named based on directory'"
Enter
Sleep 3s

# --- Scene 2: Auto-rename on cd ---
Type "echo '📂 Tab name updates when you change directory'"
Enter
Sleep 2s

Type "cd ~/Projects"
Enter
Sleep 3s

Type "cd ~/Projects/zellij-smart-tabs"
Enter
Sleep 3s

# --- Scene 3: Auto-rename on cd but shows git root name ---
Type "echo '📂 Tab name prioritises git root when you change directory'"
Enter
Sleep 2s

Type "cd ~/Projects/zellij-smart-tabs/src/"
Enter
Sleep 1s

Type "cd ~/Projects/zellij-smart-tabs/docs/"
Enter
Sleep 1s

Type "cd ~/Projects/zellij-smart-tabs/"
Enter

# --- Scene 4: Program detection ---
Type "echo 'Tab shows the running program. Note the change from   ->  '"
Enter
Sleep 2s

Type "nvim ."
Enter
Sleep 5s

Escape
Type ":q"
Enter
Sleep 5s

# --- Scene 4: Pane status via pipe ---
Type "echo '📡 Programs can report status via pipe (running → done → idle)'"
Enter
Sleep 2s

Type@30ms `zellij pipe --plugin smart-tabs --name pane_status -- "{\"pane_id\":\"$ZELLIJ_PANE_ID\",\"status\":\"running\"}"`
Enter
Sleep 3s

Type@30ms `zellij pipe --plugin smart-tabs --name pane_status -- "{\"pane_id\":\"$ZELLIJ_PANE_ID\",\"status\":\"done\"}"`
Enter
Sleep 3s

Type@30ms `zellij pipe --plugin smart-tabs --name pane_status -- "{\"pane_id\":\"$ZELLIJ_PANE_ID\",\"status\":\"idle\"}"`
Enter
Sleep 3s
