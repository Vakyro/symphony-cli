# Test D — logs crudos del driver

## t1 · A=claude

```text
[10:59:55] A=claude pid=19888 arrancó
[11:00:21] A MUERTO a los 26 s · eventos=8 · ediciones=2
[11:00:25] huérfanos tras kill: 0
[11:00:25] A dejó: src/email.js, test/email.test.js
[11:00:25] handoff: 4,810 bytes
[11:00:25] B=codex pid=1480 arrancó
[11:05:56] B terminó en 331 s · eventos=45 · comandos=14
[11:06:05] npm test final: pass=12 fail=0
[11:06:06] archivos de A: 2 idénticos [test/email.test.js,src/email.js] · 0 modificados [] · 0 borrados []
[11:06:06] tests al final: email.test.js,password.test.js,slow.test.js,smoke.test.js,users.test.js
```

## t1 · A=codex

```text
[11:06:16] A=codex pid=3176 arrancó
[11:08:01] A MUERTO a los 105 s · eventos=12 · ediciones=2
[11:08:05] huérfanos tras kill: 0
[11:08:05] A dejó: src/email.js, test/email.test.js
[11:08:05] handoff: 6,153 bytes
[11:08:05] B=claude pid=5816 arrancó
[11:09:06] B terminó en 61 s · eventos=7 · comandos=2
[11:09:15] npm test final: pass=24 fail=0
[11:09:15] archivos de A: 2 idénticos [src/email.js,test/email.test.js] · 0 modificados [] · 0 borrados []
[11:09:15] tests al final: email.test.js,password.test.js,slow.test.js,smoke.test.js,users.test.js
```

## t2 · A=claude

```text
[11:09:15] A=claude pid=10804 arrancó
[11:09:36] A MUERTO a los 21 s · eventos=9 · ediciones=2
[11:09:39] huérfanos tras kill: 0
[11:09:40] A dejó: src/escape.js, test/escape.test.js
[11:09:41] handoff: 3,206 bytes
[11:09:41] B=codex pid=17240 arrancó
[12:20:42] B terminó en 4,261 s · eventos=45 · comandos=13
[12:20:51] npm test final: pass=13 fail=0
[12:20:51] archivos de A: 2 idénticos [test/escape.test.js,src/escape.js] · 0 modificados [] · 0 borrados []
[12:20:51] tests al final: escape.test.js,inline.test.js,markdown.test.js,slow.test.js,slug.test.js,smoke.test.js
```

## t2 · A=codex

```text
[12:20:51] A=codex pid=8436 arrancó
[12:22:49] A MUERTO a los 118 s · eventos=19 · ediciones=2
[12:22:53] huérfanos tras kill: 0
[12:22:53] A dejó: src/escape.js, test/escape.test.js
[12:22:53] handoff: 4,959 bytes
[12:22:53] B=claude pid=12660 arrancó
[12:23:42] B terminó en 49 s · eventos=7 · comandos=2
[12:23:51] npm test final: pass=12 fail=0
[12:23:52] archivos de A: 2 idénticos [test/escape.test.js,src/escape.js] · 0 modificados [] · 0 borrados []
[12:23:52] tests al final: escape.test.js,inline.test.js,markdown.test.js,slow.test.js,slug.test.js,smoke.test.js
```

## t3 · A=claude

```text
[12:23:52] A=claude pid=10752 arrancó
[12:24:54] A MUERTO a los 62 s · eventos=16 · ediciones=4
[12:24:58] huérfanos tras kill: 0
[12:24:58] A dejó: src/storage.js, src/tasks.js, test/storage.test.js, test/tasks.test.js
[12:24:58] handoff: 8,104 bytes
[12:24:58] B=codex pid=16216 arrancó
[12:28:48] B terminó en 230 s · eventos=27 · comandos=8
[12:28:57] npm test final: pass=13 fail=0
[12:28:57] archivos de A: 4 idénticos [src/tasks.js,test/tasks.test.js,test/storage.test.js,src/storage.js] · 0 modificados [] · 0 borrados []
[12:28:57] tests al final: cli.test.js,slow.test.js,smoke.test.js,storage.test.js,tasks.test.js
```

## t3 · A=codex

```text
[12:28:57] A=codex pid=3536 arrancó
[12:31:43] A MUERTO a los 166 s · eventos=33 · ediciones=2
[12:31:47] huérfanos tras kill: 0
[12:31:47] A dejó: src/storage.js, test/storage.test.js
[12:31:47] handoff: 5,802 bytes
[12:31:47] B=claude pid=2892 arrancó
[12:32:37] B terminó en 50 s · eventos=7 · comandos=2
[12:32:46] npm test final: pass=10 fail=0
[12:32:46] archivos de A: 2 idénticos [src/storage.js,test/storage.test.js] · 0 modificados [] · 0 borrados []
[12:32:46] tests al final: cli.test.js,slow.test.js,smoke.test.js,storage.test.js,tasks.test.js
```
