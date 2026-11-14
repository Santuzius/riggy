#!/usr/bin/env bash
# tools/tray_menu.sh [LABEL]: prints the menu of the running riggy-tray, or clicks the entry labelled exactly LABEL (through D-Bus, as the panel would).
S=$(busctl --user list --no-legend | awk '{print $1}' | while read -r name; do pid=$(busctl --user status "$name" 2>/dev/null | sed -n "s/^PID=//p"); [ "$(cat /proc/$pid/comm 2>/dev/null)" = riggy-tray ] && [[ $name == org.kde.StatusNotifierItem-* ]] && echo "$name"; done | head -1)
[ -n "$S" ] || { echo "riggy-tray is not running"; exit 1; }
set -- "$S" "$@"
layout=$(busctl --user --json=short call $S /MenuBar com.canonical.dbusmenu GetLayout iias 0 -- -1 1 label)
if [ -z "$2" ]; then echo "$layout" | python3 -c "
import json,sys
def walk(n,d=0):
    i,p,k=n; print('  '*d,i,p.get('label',{}).get('data'))
    [walk(c['data'],d+1) for c in k]
walk(json.load(sys.stdin)['data'][1])"; exit; fi
id=$(echo "$layout" | python3 -c "
import json,sys
def walk(n):
    i,p,k=n
    if str(p.get('label',{}).get('data','')) == sys.argv[1]: print(i); sys.exit()
    [walk(c['data']) for c in k]
walk(json.load(sys.stdin)['data'][1])" "$2")
busctl --user call $S /MenuBar com.canonical.dbusmenu Event isvu $id clicked s "" 0
