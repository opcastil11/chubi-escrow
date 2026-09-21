# Runbook — deploy capa de disputa `chubi-bet` (devnet)

> **NO EJECUTADO.** Preparado en solo-lectura. Ningún deploy ni tx corrida.
> Fecha de preparación: 2026-09-21.

## 0. Hechos verificados

| Ítem | Valor | Fuente |
|------|-------|--------|
| Program ID | `CnnSaPfCLBD5V3yaYeV1bwEhUyuymdietSMiCF7HYmm8` | `Anchor.toml [programs.devnet]` + `declare_id!` en `lib.rs` |
| Cluster | `devnet` | `Anchor.toml [provider] cluster` |
| Wallet autoridad (Anchor) | `~/.config/solana/id.json` → `4wWXFYtmono7r4JivreNbbQQzrXKjy4CkzeeyWg1Aghe` | `Anchor.toml [provider] wallet` + `solana-keygen pubkey` |
| **Upgrade authority del programa desplegado** | `4wWXFYtmono7r4JivreNbbQQzrXKjy4CkzeeyWg1Aghe` | `solana program show … --url devnet` |
| ¿Coincide con keypair local? | **SÍ** — es exactamente `id.json` | comparación pubkeys |
| Balance autoridad (devnet) | 4.586 SOL | `solana balance` |
| `.so` recién compilado | 403.184 bytes | `target/deploy/chubi_bet.so` |
| `.so` desplegado actual | 369.224 bytes (más chico) | `solana program show` Data Length |
| `create_market` args (IDL nuevo) | `market_id, close_duration, num_sides, fee_recipient, creator, dispute_window` (6) | `target/idl/chubi_bet.json` |
| `create_market` args (IDL frontend viejo) | `…, creator` (5, **sin** `dispute_window`) | `frontend/src/idl/chubi_bet.json` |
| Eventos nuevos en IDL | `BetOutcomeProposed`, `BetDisputed`, `BetDisputeResolved` | `target/idl/chubi_bet.json events[]` |

> Nota sobre "7º arg": el handler Rust `create_market` recibe 7 parámetros
> **contando `ctx`**. En el IDL/cliente son **6 args** posicionales; el nuevo
> es `dispute_window` (último). Los clientes deben pasar 6 valores.

**Conclusión clave:** esto es un **UPGRADE de un programa existente**, no un
deploy nuevo. La upgrade authority está disponible localmente → se puede
actualizar sin ceremonia de rotación de autoridad.

---

## 1. Pre-flight (solo lectura, seguro)

```bash
cd /home/kai/Escritorio/PROGRAMACION/Chubi/chubi-escrow

# Confirmar cluster y wallet activos
solana config get                       # RPC = devnet, keypair = id.json
solana address                          # -> 4wWXFYtmono7r4JivreNbbQQzrXKjy4CkzeeyWg1Aghe

# Confirmar upgrade authority == wallet local (debe coincidir)
solana program show CnnSaPfCLBD5V3yaYeV1bwEhUyuymdietSMiCF7HYmm8 --url devnet

# Confirmar artefactos ya compilados
ls -l target/deploy/chubi_bet.so        # ~403 KB
ls -l target/idl/chubi_bet.json         # IDL con 6 args + 3 eventos nuevos

# Balance suficiente para el upgrade (el .so creció ~34 KB; hay 4.5 SOL, sobra)
solana balance --url devnet
```

Si la upgrade authority **no** coincidiera con la wallet local, PARAR: el
upgrade fallaría (`Error: … is not the authority`). No es el caso aquí.

---

## 2. (Opcional) recompilar determinístico

Ya compila; solo si se quiere partir de limpio antes de desplegar:

```bash
cargo build-sbf                         # -> target/deploy/chubi_bet.so
anchor build                            # -> target/idl/chubi_bet.json
```

---

## 3. Deploy del programa (UPGRADE) — IRREVERSIBLE

> Primera acción pública irreversible. **Confirmar con el usuario antes.**

Opción A — vía Anchor (usa `Anchor.toml`: cluster devnet, wallet id.json):

```bash
anchor upgrade target/deploy/chubi_bet.so \
  --program-id CnnSaPfCLBD5V3yaYeV1bwEhUyuymdietSMiCF7HYmm8 \
  --provider.cluster devnet
```

Opción B — vía Solana CLI directo (equivalente, más control de buffer):

```bash
solana program deploy target/deploy/chubi_bet.so \
  --program-id CnnSaPfCLBD5V3yaYeV1bwEhUyuymdietSMiCF7HYmm8 \
  --upgrade-authority ~/.config/solana/id.json \
  --url devnet
```

Verificar:

```bash
solana program show CnnSaPfCLBD5V3yaYeV1bwEhUyuymdietSMiCF7HYmm8 --url devnet
# Data Length debe subir a ~403 KB; "Last Deployed In Slot" nuevo.
```

Si queda un buffer huérfano por un deploy interrumpido:

```bash
solana program show --buffers --url devnet
solana program close <BUFFER_ADDR> --url devnet   # recupera el rent
```

---

## 4. Publicar / actualizar el IDL on-chain (opcional pero recomendado)

```bash
# Si ya existe IDL on-chain -> upgrade; si no, init.
anchor idl upgrade CnnSaPfCLBD5V3yaYeV1bwEhUyuymdietSMiCF7HYmm8 \
  -f target/idl/chubi_bet.json --provider.cluster devnet
# (primera vez sería:  anchor idl init … -f target/idl/chubi_bet.json)
```

---

## 5. Propagar el IDL a backend y frontend

El IDL nuevo (`target/idl/chubi_bet.json`) es la fuente de verdad. Copiarlo a
los dos consumidores:

```bash
BET_IDL=/home/kai/Escritorio/PROGRAMACION/Chubi/chubi-escrow/target/idl/chubi_bet.json
CMP=/home/kai/Escritorio/PROGRAMACION/Chubi/chubi/conviction-market-protocol

# Backend (chain-sync lo espera aquí; HOY NO EXISTE -> hay que crearlo)
cp "$BET_IDL" "$CMP/backend/src/chain-relay/chubi_bet.json"

# Frontend (ya existe la versión vieja de 5 args -> sobreescribir)
cp "$BET_IDL" "$CMP/frontend/src/idl/chubi_bet.json"
```

> Verificado: `backend/src/chain-relay/chubi_bet.json` **no existe** todavía —
> `chain-sync.js` lo carga condicionalmente y hoy logea *"missing — skipping
> bet sync"*. Al copiarlo se habilita la sincronización de eventos bet.
> El `frontend/src/idl/chubi_bet.json` **sí existe** pero es la versión vieja
> (5 args, sin los 3 eventos de disputa) → hay que pisarlo.

Verificar que ambos tienen 6 args y los 3 eventos:

```bash
for f in "$CMP/backend/src/chain-relay/chubi_bet.json" "$CMP/frontend/src/idl/chubi_bet.json"; do
  echo "== $f =="
  python3 -c "import json,sys;d=json.load(open(sys.argv[1]));print('args', [a['name'] for x in d['instructions'] if x['name']=='create_market' for a in x['args']]);print('has dispute events', all(e in [ev['name'] for ev in d['events']] for e in ['BetOutcomeProposed','BetDisputed','BetDisputeResolved']))" "$f"
done
```

---

## 6. Cambios de código — BACKEND

### 6a. Relay: pasar `dispute_window` a `create_market`

**Aclaración importante:** el `createMarketOnChain` que existe en
`backend/src/chain-relay/SolanaRelay.js` es el de **chubi-escrow** (carga
`chubi_escrow.json`, su `createMarket` toma `allowWithdrawal`/`enableLockout`).
**No** es el relay del programa bet. En el backend actual **no hay** un relay
que cree mercados bet on-chain: los mercados bet on-chain se crean desde el
**frontend** (`CreateMatch.jsx → createBetMarketOnChain`). El backend solo los
**lee** vía ChainSync.

→ Acción backend sobre creación:
- **Si** existe/aparece un camino server-side que llame al `create_market` de
  chubi-bet (script de seed `backend/scripts/e2e-bet-devnet.js`, o un futuro
  BetRelay), agregar el 6º arg `dispute_window` (BN, segundos; 0 = sin
  disputa) a la llamada `.createMarket(marketId, duration, numSides,
  feeRecipient, creator, new BN(disputeWindowSecs))`.
- La API ya calcula `disputeWindowSecs` (`routes.js` ~L302-314, cap 7 días) y
  lo persiste; queda propagarlo al punto de creación on-chain que corresponda.

### 6b. ChainSync: decoders de los 3 eventos nuevos

**Verificado:** el `switch` de `ChainSync.js` (~L271-289) NO tiene casos para
`BetOutcomeProposed`, `BetDisputed`, `BetDisputeResolved` → hoy caen en el
`default` ("unknown event") y se descartan silenciosamente. Hay que:

1. Agregar 3 `case` al switch, junto a los otros `Bet*`:

   ```js
   case 'BetOutcomeProposed':   await this._onBetOutcomeProposed(data, sig); break;
   case 'BetDisputed':          await this._onBetDisputed(data, sig); break;
   case 'BetDisputeResolved':   await this._onBetDisputeResolved(data, sig); break;
   ```

2. Implementar los 3 handlers en la sección "chubi-bet event handlers"
   (~L602). Campos disponibles por evento (del IDL/`events.rs`):
   - `BetOutcomeProposed`: `market_id, winner (u8, 1-idx), proposed_at (i64),
     dispute_window (i64), proposer (Pubkey)`
   - `BetDisputed`: `market_id, disputer (Pubkey), bond (u64),
     disputed_winner (u8)`
   - `BetDisputeResolved`: `market_id, winner (u8), upheld (bool),
     bond_returned_to (Pubkey), bond_amount (u64)`

   Mapear al estado de mercado (`markets.status`/columnas de resolución) igual
   que hacen los adapters existentes (`_onBetMarketResolved` reusa
   `_onMarketResolved`). `BetDisputeResolved` debe converger al mismo estado
   final que un `BetMarketResolved` (winner + resolved). `BetOutcomeProposed`
   marca "proposed"; `BetDisputed` marca "disputed" (hay estados UI análogos en
   `OperatorDashboard.jsx`: proposed/disputed/resolved).

3. (Si aplica) migración SQL para columnas de disputa si se quiere persistir
   `proposed_winner/proposed_at/disputer/dispute_bond` a nivel fila. Si solo se
   refleja el estado final, puede bastar con el status existente.

> El resto de la tubería bet (BetMarketCreated/BetPlaced/… → tablas
> `markets/orders/claims`, `mode='bet'`) ya funciona y no cambia.

### 6c. Env

Asegurar `BET_PROGRAM_ID=CnnSaPfCLBD5V3yaYeV1bwEhUyuymdietSMiCF7HYmm8` en el
entorno del backend para que `chain-sync.js` arranque el sync del programa bet
(hoy es opcional; sin él no se sincroniza nada de bet).

---

## 7. Cambios de código — FRONTEND

**Verificado:** `frontend/src/pages/CreateMatch.jsx → createBetMarketOnChain`
(~L90) llama `.createMarket(marketId, new BN(durationSeconds), numSides,
FEE_RECIPIENT, authority)` — **5 args, sin `dispute_window`**. El form ya tiene
`disputeWindowMins` (state, L263). Hay que:

1. Aceptar `disputeWindowSecs` en las `opts` de `createBetMarketOnChain`.
2. Pasar el 6º arg:
   ```js
   .createMarket(marketId, new BN(durationSeconds), numSides,
                 FEE_RECIPIENT, authority, new BN(disputeWindowSecs))
   ```
3. Cablear `disputeWindowMins * 60` desde el form al call (L350 arma `opts`).
4. El `chubi_bet.json` del frontend ya se sobrescribió en el paso 5 → Anchor
   toma el layout de 6 args del IDL, así que sin (1)-(3) el build compila pero
   la llamada revierte por args faltantes.

(Opcional) UI de disputa: `BetMarketView.jsx` ya tiene el flujo de dispute
off-chain (POST /dispute). Si se quiere disputa on-chain directa, agregar
llamadas a `propose_outcome`/`dispute`/`finalize`/`resolve_disputed` — no es
requisito del deploy, se puede iterar después.

---

## 8. Verificación post-deploy

```bash
# Programa
solana program show CnnSaPfCLBD5V3yaYeV1bwEhUyuymdietSMiCF7HYmm8 --url devnet   # nuevo slot, ~403KB

# Backend: arrancar y ver que el sync bet levanta (ya no "missing")
#   log esperado: "[chain-sync] running (bet) against Cnn…"
# Frontend: build
cd "$CMP/frontend" && npm run build

# E2E devnet (script existente) — crea un mercado bet con dispute_window>0,
# propone, (opcional) disputa, finaliza:
node "$CMP/backend/scripts/e2e-bet-devnet.js"   # revisar/ajustar para pasar dispute_window
```

Chequear en logs de ChainSync que los eventos `BetOutcomeProposed` /
`BetDisputed` / `BetDisputeResolved` ya **no** salen como "unknown event".

---

## 9. Riesgos y rollback

### Es upgrade, no deploy nuevo
- Mismo program ID; los mercados bet existentes siguen bajo el mismo PDA.
- Upgrade authority local ⇒ reversible re-desplegando el `.so` anterior si se
  guarda copia. **Antes del upgrade, respaldar el binario actual:**
  ```bash
  solana program dump CnnSaPfCLBD5V3yaYeV1bwEhUyuymdietSMiCF7HYmm8 \
    /tmp/chubi_bet_PREV.so --url devnet
  ```
  Rollback: `solana program deploy /tmp/chubi_bet_PREV.so --program-id … \
  --upgrade-authority ~/.config/solana/id.json --url devnet`.

### Layout de `BetMarket` (campos nuevos al final)
- Los 5 campos de disputa (`dispute_window, proposed_winner, proposed_at,
  disputer, dispute_bond`) se agregaron **al final** de la struct → los
  offsets de los campos previos no se mueven.
- **RIESGO REAL:** las cuentas `BetMarket` ya existentes en devnet fueron
  creadas con el `space` viejo (más chico). El programa nuevo intentará
  deserializar más bytes de los que la cuenta tiene ⇒ los mercados **viejos**
  pueden fallar a leer/escribir (`AccountDidNotDeserialize` / fuera de rango)
  porque Anchor `InitSpace` cambió y esas cuentas no se realocan solas.
  - Mitigación A (recomendada en devnet): tratar los mercados bet viejos como
    legacy — no operarlos con el binario nuevo; crear mercados nuevos (que
    nacen con el `space` nuevo vía `init`).
  - Mitigación B: si hay que preservarlos, agregar una instrucción de
    migración/realloc (no existe hoy) — fuera de alcance de este runbook.
  - En devnet el set de mercados bet reales es descartable; A es lo pragmático.

### Desalineación IDL ↔ frontend (el punto fino)
- **Programa actualizado (6 args) + frontend viejo (5 args):** las llamadas
  `create_market` del frontend **revierten** (faltan args / borsh mismatch).
  Los mercados bet no se pueden crear desde la app hasta actualizar frontend.
- **Frontend actualizado (6 args) + programa viejo (5 args):** también
  revierte (el programa viejo no conoce el 6º arg). 
- ⇒ **Programa y frontend deben desplegarse juntos** (o al menos el frontend
  inmediatamente después del upgrade). No dejar la ventana abierta.
- Backend/ChainSync es tolerante: el IDL nuevo decodifica eventos viejos y
  nuevos; el único gap es que sin los 3 `case` nuevos los eventos de disputa se
  descartan (no rompe, pero se pierde estado). Con dispute_window=0 el flujo
  viejo (`resolve_market`) sigue igual.

### Orden de operaciones seguro (minimiza ventana rota)
1. Backup del `.so` actual (paso 9).
2. Copiar IDL a backend + frontend (paso 5).
3. Aplicar cambios de código backend (6b/6c) y frontend (7).
4. **Upgrade del programa** (paso 3) — irreversible, confirmar.
5. Publicar IDL on-chain (paso 4).
6. Desplegar frontend nuevo y reiniciar backend **inmediatamente**.
7. Verificar (paso 8).

> Regla: crear mercados con `dispute_window=0` primero para validar que el
> upgrade no rompió el camino clásico, antes de probar el flujo de disputa.
