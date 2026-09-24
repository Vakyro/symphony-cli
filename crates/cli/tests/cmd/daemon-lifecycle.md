# Ciclo de vida del daemon

```console
$ symphony daemon status
daemon: detenido

$ symphony daemon start
daemon: iniciado (pid [..])

$ symphony daemon start
daemon: ya estaba corriendo (pid [..])

$ symphony status
daemon: corriendo
  pid:      [..]
  versión:  [..]
  activo:   [..]
  home:     [..]

$ symphony daemon stop
daemon: detenido

$ symphony daemon stop
daemon: no estaba corriendo

```

`symphony status` arranca el daemon si no está vivo:

```console
$ symphony status
daemon: corriendo
  pid:      [..]
  versión:  [..]
  activo:   [..]
  home:     [..]

$ symphony daemon stop
daemon: detenido

```
