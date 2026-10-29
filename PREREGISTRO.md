# Prerregistro de la medición / Measurement pre-registration

Escrito y commiteado antes de correr el barrido. / Written and committed before running the sweep.

**Divulgación / disclosure:** antes de este commit sólo corrieron los tests (`cargo test`, 60 requests por corrida en 5 configuraciones). / Only the tests (60 requests per run, 5 configurations) ran before this commit.

Comando / command:

```sh
cargo run --release --bin dpop-replay-mesh -- --requests 1000 --seed 42 --csv results/sweep.csv --check
```

Dos corridas: la segunda en un clon limpio. Las columnas de conteo (`legit_ok`, `nonce_retries`, `replays_sent`, `replays_accepted`) tienen que ser idénticas, porque las ventanas usan tiempo simulado. La latencia es de reloj real y va a variar. / Two runs, the second in a clean clone; count columns must be identical, latency will vary.

Parámetros: ventana de `iat` ±10 s; un request legítimo cada 10 ms simulados; un replay por request aceptado, a un nodo al azar, con retardo uniforme en [0, 20 s]; desfase de cada nodo uniforme en [−skew, +skew].

| Modo | Esperado replays aceptados | Criterio |
|---|---|---|
| `no_check` (control negativo) | ≈ 50 % (los replays con retardo ≤ ventana) | > 0 en todas las configuraciones |
| `local` | 0 con N = 1; ≈ 50 % × (N−1)/N con N > 1 | N = 1 → 0; el resto se reporta |
| `shared_ttl_w` | 0 con skew 0; > 0 con skew > 0 (un nodo atrasado acepta un replay que ya salió de la caché) | se reporta |
| `shared_ttl_w_plus_skew` | 0 | 0 en todas |
| `nonce` | 0 | 0 en todas |

Latencia: p50/p99 del lado del cliente (firma ES256 + TCP loopback + verificación + reintento de nonce). Hipótesis: con N > 1 el nonce casi siempre exige un reintento (el nonce lo emitió otro nodo), así que su p99 ≈ 2× el de `local`. Criterio del handoff: `nonce` con 0 aceptados y p99 < 2× el de `local` en la misma (N, skew); se reporta cumplido o no por configuración. También se reporta el p99 de `nonce` contra `shared_ttl_w_plus_skew`.

Todos los requests legítimos tienen que aceptarse (skew ≤ 5 s < ventana 10 s); `--check` falla si no.

Límites conocidos / known limits: un solo cliente secuencial, sin carga concurrente; loopback en una VM compartida, no red real; el nonce se liga al nodo que lo emite y el balanceo es al azar (con sesiones pegajosas el reintento desaparecería, no se mide); no hay access token ni `ath`.
