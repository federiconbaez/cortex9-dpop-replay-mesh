# dpop-replay-mesh

Protección contra replay de pruebas DPoP ([RFC 9449](https://www.rfc-editor.org/rfc/rfc9449)) cuando hay N servidores de recursos detrás del mismo endpoint y sus relojes están desfasados. Compara caché `jti` local, caché compartida y nonce del servidor. Usa pruebas ES256 reales y TCP loopback real entre cliente, nodos y caché. Las ventanas de tiempo se simulan; la latencia se mide con el reloj real.

DPoP proof replay protection across N resource servers with clock skew: local jti cache vs shared cache vs server nonces. Real ES256 proofs and real loopback TCP, simulated time for the freshness windows, measured wall-clock latency.

> **ES** primero · **EN** abajo. Nivel de verificación: **B** (ver abajo).

---

## ES

### Qué hay

| Pieza | Qué hace |
|---|---|
| `src/proof.rs` | Crea y verifica pruebas DPoP: `typ: dpop+jwt`, ES256, `jwk` en el header, `jti`/`htm`/`htu`/`iat`/`nonce`. |
| `src/mesh.rs` | Cliente, N nodos (un hilo cada uno) y caché compartida con `SETNX` atómico, todo por TCP. Seis modos (abajo). |
| `src/bin/dpop-replay-mesh.rs` | Barrido N ∈ {1,2,4,8,16} × skew ∈ {0,1,2,5} s × modo; CSV y `--check`. |
| `tests/mesh.rs` | Ida y vuelta de la prueba, prueba alterada y los controles en corridas de 60 requests. |

Modos: `no_check` (sin chequeo de `jti`, control negativo), `local` (cada nodo con su caché; el jti vence a llegada + ventana), `local_ttl_iat` (cada nodo; el jti vence a `iat` + ventana), `shared_ttl_w` (caché compartida, TTL = ventana), `shared_ttl_w_plus_skew` (TTL = ventana + skew) y `nonce` (nonce de un solo uso; sólo lo acepta el nodo que lo emitió, RFC 9449 §9).

Parámetros: ventana de `iat` de ±10 s y un request cada 10 ms simulados. Por cada request aceptado se manda un replay a un nodo al azar, con retardo uniforme en [0, 20 s]. El desfase de cada nodo es uniforme en [−skew, +skew]. El balanceo es al azar.

### Medición

Prerregistrada en [`PREREGISTRO.md`](PREREGISTRO.md), con una adenda escrita después de la primera corrida.

```sh
cargo run --release --bin dpop-replay-mesh -- --requests 1000 --seed 42 --csv results/sweep.csv --check
```

**Primera corrida (`results/run1-prereg/`): falló un control prerregistrado.** Con un solo nodo, la caché `local` aceptó 36 replays (3,6 %) con skew de 1 s y 44 (4,4 %) con skew de 2 s. El jti vencía a *llegada + ventana*, pero la ventana de `iat` de un nodo atrasado sigue abierta hasta *iat + ventana + atraso*. RFC 9449 §11.1 dice que el jti se guarde «por la ventana en la que esa prueba sería aceptada»; `local_ttl_iat` hace eso y se agregó en la adenda.

**Segunda corrida (`results/sweep.csv`), `--check` = 0.** Las columnas de conteo de los cinco modos originales son idénticas a la primera corrida en las 100 configuraciones, y otra vez en un clon limpio.

Replays aceptados, en % de 1000 (skew 0 / 1 / 2 / 5 s):

| N | no_check | local | local_ttl_iat | shared_ttl_w | shared_ttl_w_plus_skew | nonce |
|---|---|---|---|---|---|---|
| 1 | 50,0 / 47,3 / 42,3 / 68,7 | 0 / **3,6** / **4,4** / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / **6,9** | 0 | 0 |
| 2 | 48,2 / 44,2 / 53,5 / 31,4 | 24,6 / 25,6 / 27,1 / 19,7 | 24,3 / 23,9 / 21,1 / 23,3 | 0 / 2,7 / 4,3 / 11,9 | 0 | 0 |
| 4 | 49,7 / 47,2 / 47,2 / 50,9 | 38,5 / 37,1 / 41,9 / 30,8 | 37,8 / 33,6 / 38,8 / 41,2 | 0 / 2,0 / 3,3 / 6,4 | 0 | 0 |
| 8 | 49,5 / 51,3 / 46,8 / 52,4 | 44,1 / 44,8 / 43,5 / 42,0 | 43,1 / 44,3 / 45,4 / 43,8 | 0 / 1,2 / 2,8 / 3,8 | 0 | 0 |
| 16 | 50,9 / 50,7 / 49,5 / 45,2 | 46,0 / 47,2 / 46,7 / 44,8 | 49,1 / 47,2 / 44,4 / 48,0 | 0 / 1,6 / 0,5 / 10,4 | 0 | 0 |

Latencia de `nonce` contra `local`, p50/p99 (cociente, segunda corrida; skew 0 / 1 / 2 / 5 s):

| N | skew 0 | 1 s | 2 s | 5 s |
|---|---|---|---|---|
| 1 | 0,99 / 0,95 | 1,08 / 1,38 | 0,98 / 0,48 | 1,07 / 1,06 |
| 2 | 1,77 / 0,86 | 1,79 / **4,39** | 1,48 / 1,61 | 1,75 / 1,81 |
| 4 | 1,61 / 1,35 | 2,09 / 1,83 | 1,79 / 1,59 | 1,97 / 1,81 |
| 8 | 1,84 / 1,70 | 2,00 / 1,90 | 2,06 / 1,89 | 1,72 / 1,69 |
| 16 | 1,82 / 1,75 | 1,90 / 1,89 | 2,19 / 1,29 | 1,84 / 1,91 |

Valores absolutos (segunda corrida): `local` p50 entre 0,40 y 0,57 ms, p99 entre 0,63 y 1,53 ms. `nonce` con N = 16: p50 entre 0,99 y 1,07 ms.

Lectura:
- **Criterio del handoff** (nonce con 0 aceptados y p99 < 2× local): 0 aceptados en las 20 combinaciones (N, skew) de las dos corridas. p99 < 2× en 19/20 (segunda corrida) y 18/20 (primera). Las excepciones (4,39 y 4,09) no se repiten en la otra corrida, así que parecen ruido de la VM, pero no se puede descartar otra causa. **El p50 sí se acerca a 2× con N ≥ 4**: con balanceo al azar, casi todo request trae un nonce de otro nodo y necesita un reintento (por ejemplo, 946 reintentos de 1000 con N = 16).
- Caché local: con N > 1 deja pasar ≈ (N−1)/N de los replays que caen en la ventana. Es el caso que el RFC 9449 §11.1 marca como «no siempre factible» sin estado compartido.
- Caché compartida: con TTL = ventana deja pasar hasta 11,9 % de replays cuando hay skew; con TTL = ventana + skew, 0 en todas.
- Ningún request legítimo fue rechazado en las 120 configuraciones.

### Tests

`cargo test`: 7/7. Ningún test se saltea.

### Nivel de verificación

**B**: el mecanismo está medido, con control negativo (`no_check`) y controles de 0 replays, y los conteos son deterministas en tres corridas. No es B+ porque no hay línea base externa que superar. No es C porque no hubo carga concurrente ni red real.

### Límites

- Un solo cliente secuencial, sin concurrencia. La latencia es la de TCP loopback en una VM compartida, no la de una red. Los p99 de 1000 muestras varían entre corridas.
- El tiempo de las ventanas es simulado, y el desfase de los nodos es fijo durante cada corrida (no hay deriva).
- El nonce queda ligado al nodo que lo emite y el balanceo es al azar. Con sesiones pegajosas o nonces sin estado compartidos, el reintento cambiaría; eso no se midió.
- No hay access token ni `ath`, ni HTTP real. La caché compartida es un hilo con un `HashMap`, no Redis.
- netem no se usó: hace falta root.

### Fuentes leídas

- RFC 9449, secciones 4.3, 9 y 11.1: `https://www.rfc-editor.org/rfc/rfc9449.txt`.
- API de crates.io para las versiones de `p256`: `https://crates.io/api/v1/crates/p256`.

---

## EN

### What is here

Real ES256 DPoP proofs; a client, N node threads and a shared cache with atomic `SETNX`, all over loopback TCP; six modes (`no_check`, `local`, `local_ttl_iat`, `shared_ttl_w`, `shared_ttl_w_plus_skew`, `nonce`); a sweep over N ∈ {1,2,4,8,16} × skew ∈ {0,1,2,5} s. The `iat` window is ±10 s. Each accepted request is replayed once to a random node after a uniform 0–20 s delay.

### Measurement

**The first pre-registered run falsified one control**: a single node with a jti cache that expires at arrival + window accepted 3.6 % and 4.4 % of replays at 1 s and 2 s skew. A lagging node's `iat` window outlives that entry. RFC 9449 §11.1 says to keep the jti for the window in which the proof would be accepted; `local_ttl_iat` does that and was added by a pre-registered addendum. In the second run, `--check` passes and the count columns repeat the first run exactly (and again in a clean clone).

Results are in the tables above. Nonce and the shared cache with a skew margin accepted 0 replays everywhere. A shared cache with TTL = window leaks up to 11.9 % under skew. Local caches leak about (N−1)/N of the in-window replays. Nonce p99 stays under 2× local in 19/20 and 18/20 (N, skew) cells across the two runs, with two non-repeating outliers. Nonce p50 approaches 2× at N ≥ 4 because almost every request needs a retry under random load balancing.

### Verification level

**B**: mechanism measured with negative and zero-replay controls, and the counts are deterministic across three runs. Not B+ (no external baseline), not C (no concurrency, no real network).

### Limits

Single sequential client; loopback on a shared VM; simulated time; fixed node offsets; node-bound nonces with random balancing; no access token, `ath` or HTTP; no netem (needs root).

## License

Apache-2.0.
