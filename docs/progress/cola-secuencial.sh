#!/bin/bash
# Cola secuencial: cada linea "modelo|respaldo|titulo|tarea". Spawn, espera, integra por cherry-pick.
cd "/c/Users/Latitude 7390/desktop/proyectos/symphony-cli"
S="$HOME/.symphony-stable/bin/symphony.exe"; R="$(cat .symphony/tmp/rules.txt)"; LOG=.symphony/tmp/drive.log
Q="$1"
ev(){ echo "EVT $(date +%H:%M) $*" >> "$LOG"; }
state(){ "$S" inspect "$1" 2>/dev/null | grep -m1 -E '^\s+estado' | sed 's/^ *estado: *//'; }
run(){ # modelo titulo tarea -> imprime numero de agente
  out=$("$S" spawn -m "$1" --failover any -d "$R" "$3" 2>&1); n=$(echo "$out" | grep -o 'agent-0*[0-9]*' | head -1 | sed 's/agent-0*//')
  [ -z "$n" ] && { ev "SPAWNFAIL $2 :: $(echo "$out" | tail -1)"; return 1; }
  ev "START #$n $1 $2"
  sleep 20; w=0
  while st=$(state "$n"); echo "$st" | grep -qE '^(RUNNING|CREATED|STARTING|WAITING|TESTING)'; do
    sleep 60
    if echo "$st" | grep -q '^WAITING_PROVIDER'; then w=$((w+1)); else w=0; fi
    if [ $w -gt 20 ]; then "$S" kill "$n" >/dev/null 2>&1; ev "KILL #$n sin proveedor 20 min"; break; fi
  done
  echo "$n"
}
integrate(){ # n titulo
  d=$(ls -d .symphony/worktrees/*/agent-$(printf %03d "$1") 2>/dev/null)
  [ -z "$d" ] && return 1
  wt=$(git -C "$d" rev-parse HEAD)
  cs=$(git rev-list --reverse --cherry-pick --right-only --no-merges HEAD...$wt)
  [ -z "$cs" ] && return 1
  if git cherry-pick $cs >/dev/null 2>&1; then ev "INTEGRADO #$1 $2 -> $(git log --oneline -1 | cut -c1-70)"; return 0
  else git cherry-pick --abort; ev "CONFLICT #$1 $2 (commits $cs)"; exit 3; fi
}
while IFS="|" read -r -u 3 m fb t task; do
  [ -z "$m" ] && continue
  n=$(run "$m" "$t" "$task") || continue
  integrate "$n" "$t"; rc=$?
  st=$(state "$n" | cut -c1-200)
  if [ $rc -ne 0 ] && [ "$st" = "COMPLETED" ]; then ev "SIN-CAMBIOS #$n $t (COMPLETED sin commit)"; continue; fi
  if [ $rc -ne 0 ]; then
    ev "FALLO #$n $t :: $st"
    d=$(ls -d .symphony/worktrees/*/agent-$(printf %03d "$n") 2>/dev/null); pre=""
    if [ -n "$d" ] && [ -n "$(git -C "$d" status --porcelain)" ]; then
      git -C "$d" add -A && git -C "$d" commit -q -m "wip: $t (agente $n, sin verificar)"; pre="Primero: git cherry-pick $(git -C "$d" rev-parse --short HEAD) (trabajo parcial de otro agente, sin verificar), luego terminalo y verificalo. "
    elif [ -n "$d" ] && [ -n "$(git rev-list --cherry-pick --right-only HEAD...$(git -C "$d" rev-parse HEAD))" ]; then
      pre="Primero: git cherry-pick $(git rev-list --reverse --cherry-pick --right-only HEAD...$(git -C "$d" rev-parse HEAD) | tr '\n' ' ')(trabajo parcial), luego terminalo. "
    fi
    n2=$(run "$fb" "$t (reintento)" "${pre}Motivo del fallo anterior: ${st}. TAREA: $task") || continue
    integrate "$n2" "$t (reintento)" || ev "FALLO2 #$n2 $t :: $(state "$n2" | cut -c1-200)"
  fi
done 3< "$Q"
ev "COLA-FIN $Q"
