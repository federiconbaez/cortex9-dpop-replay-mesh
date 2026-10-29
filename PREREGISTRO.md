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

## Adenda 1 / Addendum 1 (después de la primera corrida / after the first run)

Escrita y commiteada antes de la segunda corrida. / Written and committed before the second run.

La primera corrida (`results/run1-prereg/sweep.csv`) **falsó** un esperado: la caché `local` con N = 1 aceptó 36 replays con skew 1 s y 44 con skew 2 s (`--check` salió con 1). Causa: el jti vence a *llegada + ventana* en el reloj del nodo, mientras que la ventana de `iat` sigue aceptando hasta *iat + ventana + atraso del nodo*. Con skew 0 y 5 s no pasó porque el desfase sorteado para ese único nodo fue 0 o positivo.

The first run **falsified** an expectation: `local` with N = 1 accepted replays at 1 s and 2 s skew, because the jti expires at arrival + window on the node clock while the `iat` window stays open until iat + window + node lag.

Cambios para la segunda corrida / changes for the second run:
- Modo nuevo `local_ttl_iat`: el jti vence a `iat + ventana` (reloj del nodo). Esperado: 0 replays con N = 1 para todo skew; con N > 1 ≈ igual a `local`, porque entre nodos no comparte nada.
- `--check`: el control de N = 1 pasa a `local_ttl_iat`. Los demás controles no cambian.
- Las semillas de los cinco modos originales son las mismas: sus columnas de conteo tienen que repetir la primera corrida.
- Se escribe `results/latency_ratio.csv` con p99 de `nonce` / `local` y / `shared_ttl_w_plus_skew`.

Comando / command:

```sh
cargo run --release --bin dpop-replay-mesh -- --requests 1000 --seed 42 --csv results/sweep.csv --check
```
