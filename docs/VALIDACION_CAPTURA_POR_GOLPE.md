# Captura por golpe: qué está probado y qué falta validar

La captura por golpe (Ayudas → *Captura por golpe*, por defecto desde C6)
mide cada golpe de tecla entero y deja el resultado fijo hasta el próximo.

> **Importante:** todas las pruebas automáticas usan señales simuladas
> (un modelo ideal de cuerda: sin cuerdas falsas, sin acople entre
> cuerdas, sin eco de la sala, sin micrófonos con control automático de
> volumen). Que pasen **no demuestra** que el algoritmo funcione bien en un
> piano real. Lo de abajo marcado como *pendiente* necesita grabaciones.

## Cómo funciona hoy

1. El hilo del micrófono detecta el golpe por un salto de volumen (9 dB
   sobre el mínimo desde el golpe anterior y 12 dB sobre el ruido de fondo,
   sin dos golpes en menos de 150 ms).
2. Ignora los primeros 100 ms (40 ms de martillo + 60 ms de ventana de
   análisis) y junta las frecuencias crudas hasta que el volumen cae 30 dB,
   hay silencio, pasan 1,5 s o llega otro golpe.
3. Resultado: mediana, dispersión (rango intercuartil), cantidad de
   lecturas y cuánto sobresalió sobre el ruido.
4. Clasificación en la app:
   - **0 lecturas** → no fue la nota (llave, voz, ruido): se ignora.
   - **1–5 lecturas** → *aproximado*: se muestra, no cuenta para avanzar.
   - **dispersión > 3 cents** → *desparejo*: hay que repetir.
   - resto → **válido**.
5. Avance automático: los **dos últimos golpes válidos** dentro del margen
   **y** a menos de `max(1 cent, margen/2)` entre sí.
6. Unísonos agudos: cada cuerda se afina **sola** (la ya afinada apagada)
   contra **lo que midió la primera cuerda**. Al final se controla a oído.

## Probado (código + simulación)

| Afirmación | Prueba |
|---|---|
| Ruido o golpe de llave dan 0 lecturas | `detector_tests::noise_burst_gives_no_readings` |
| Un C8 suave con pocas lecturas igual mide bien | `detector_tests::weak_short_treble_strike_still_measures` |
| Con dos cuerdas juntas se mide una mezcla y la dispersión casi no sube (por eso el unísono se afina con la cuerda sola) | `detector_tests::two_strings_together_blend` |
| Clasificación de golpes | `app::tests::strike_classes` |
| Avance: dos golpes en zona que no coinciden no avanzan | `app::tests::advances_only_with_two_agreeing_strikes_in_the_zone` |
| Dos golpes seguidos no se mezclan; sonido continuo no es golpe | `strike::tests::*` |

En el laboratorio de simulación además se vio:
- el detector varía 0,02–0,07 cents entre golpes iguales y tiene un sesgo
  fijo de unos −0,3 cents con cuerdas inarmónicas (se cancela al comparar
  cuerdas entre sí);
- saltear 100 ms elimina una subida de +6 cents en el ataque;
- recortar la ventana a 100–600 ms no cambiaba nada (por eso **no** se
  hizo).

## Pendiente de validar con grabaciones de un piano real

1. **Variación real entre golpes** de una cuerda sin tocar (golpes suaves,
   medios y fuertes). Con eso se calibra la tolerancia entre los dos golpes
   del avance (hoy `max(1, margen/2)`, sin base medida) y el límite de
   dispersión de 3 cents.
2. **Cuerdas falsas** en los agudos: una sola cuerda puede "batir" sola.
   Puede dar golpes *desparejos* aunque la cuerda esté bien.
3. **Deriva de altura durante el golpe**: comparar la mediana de 100–250 ms
   con la del resto. Si difiere más de 0,3 cents de forma sistemática,
   fijar la ventana en el tramo temprano.
4. **Si 100 ms de salteo es necesario** en C8 (mirar las lecturas de
   60–100 ms).
5. **Control del unísono con las cuerdas juntas** por la curva de volumen
   (en simulación separa errores de 2 cents o más en C6–C7, pero en C8 una
   sola cuerda ya muestra modulación). Hoy **no está implementado**: el
   control final es a oído.
6. **Micrófonos con cancelación de ruido o control automático de volumen**,
   eco de la sala, golpes de llave reales y voces.
7. **Umbrales de detección del golpe** (9 / 12 dB) con micrófonos y
   distancias distintas.

### Grabaciones que hacen falta

WAV, 48 kHz, con el mismo micrófono y posición que al afinar. Nombre del
archivo con lo que contiene (ej. `C7_sola_fuerte.wav`).

- C6, C7, C8 (y alguna intermedia): **una cuerda sola**, 10 golpes suaves,
  10 medios y 10 fuertes, **sin tocar la clavija**.
- **Unísonos desafinados a propósito** una cantidad conocida (medir cada
  cuerda sola antes): ±1, ±3 y ±6 cents. Y unísonos limpios afinados a
  oído.
- Golpes de la llave contra la clavija, voces y ruido normal de la sala.

Para usarlas en pruebas automáticas conviene que el detector de golpes
mida el tiempo por muestras de audio y no con el reloj de la compu
(cambio pendiente).
