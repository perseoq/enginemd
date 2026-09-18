# Modo daemon

`enginemd daemon` ejecuta el servidor en segundo plano y lo mantiene activo
tras reiniciar la máquina, sin necesidad de configurar un servicio a mano.

## Comandos

```bash
enginemd daemon start      # arranca en segundo plano y activa el autoarranque
enginemd daemon status     # estado del proceso y del autoarranque
enginemd daemon logs       # últimas líneas del log
enginemd daemon stop       # detiene el daemon y desactiva el autoarranque
enginemd daemon restart    # stop + start
```

Los flags globales del servidor se reenvían al proceso en segundo plano:

```bash
enginemd --port 8080 daemon start
enginemd daemon start --port 8080 --path /ruta   # también válido (flags globales)
```

## Cómo funciona

- **Proceso**: `daemon start` relanza el binario como servidor normal, con
  `setsid` para desacoplarlo de la terminal, `stdin` cerrado y `stdout`/`stderr`
  redirigidos al log.
- **PID**: `~/.enginemd/enginemd.pid`
- **Log**: `~/.enginemd/enginemd.log` (se añade, no se sobrescribe)
- **Readiness**: tras arrancar espera hasta 5 s a que el puerto acepte
  conexiones; si no, avisa y deja el detalle en el log.
- **Autoarranque**: añade un bloque `@reboot` al crontab del usuario:

  ```
  # enginemd-daemon
  @reboot /ruta/al/enginemd daemon start >/dev/null 2>&1
  ```

  `daemon stop` elimina ese bloque. Las demás entradas del crontab se conservan.
  Al arrancar por cron, `daemon start` es idempotente (no duplica procesos).

## Seguridad y robustez

- `daemon stop` solo termina el PID gestionado en el pidfile; nunca toca otros
  procesos.
- Si el PID del pidfile ya no corresponde a un `enginemd` (p. ej. tras un
  reinicio), se considera obsoleto y se limpia.
- Si `crontab` no está disponible, el daemon arranca igualmente y solo se avisa
  de que no habrá persistencia.
- Soporte solo Unix (Linux/macOS). En otras plataformas devuelve un error.
