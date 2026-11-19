# Piano Tuner Free 🎹

**Afinador de piano gratuito y open source para Windows.** Afiná tu piano con el micrófono de tu notebook: te guía nota por nota, cuerda por cuerda, con detector de batido para los unísonos.

🇬🇧 [Read in English](README.md)

## ⬇️ Descargar

**[Descargar PianoTunerFree.exe](https://github.com/mateoherrerae/pianoTunerFree/raw/main/dist/PianoTunerFree.exe)** (Windows 10/11, ~7 MB, no necesita instalación)

Abrilo con doble clic. Si Windows SmartScreen muestra un aviso, tocá **Más información → Ejecutar de todas formas** (la app no está firmada digitalmente).

## Qué hace

- **La central configurable**: 440, 442, 435 o cualquier valor, por ejemplo 417 Hz para un piano viejo que conviene no subir de golpe.
- **Cuerdas de tu piano**: elegís en qué nota empiezan las 2 cuerdas y en cuál las 3; los pasos se adaptan (apagar las de los costados, afinar la del medio, después los unísonos).
- **Detector de batido en los unísonos**: muestra los batidos por segundo y te avisa cuando el unísono queda limpio. Puede avanzar solo.
- **Medir mi piano**: tocás 10 teclas y la app calcula el estiramiento según la inharmonicidad de *tu* piano.
- **Graves por armónicos**: debajo de C3 mide con los armónicos, porque los micrófonos de notebook casi no captan la nota base.
- **Mantiene la última lectura** para los agudos que se apagan rápido.
- **Niveles**: Básico / Intermedio / Avanzado, o activás cada ayuda por separado.
- **Varios pianos guardados**, elección de micrófono con medidor de volumen y opción de retomar una afinación a la mitad.
- **Afinador libre** (cromático), teclado de 88 teclas clickeable y tono de referencia.
- **Detectar el La de tu piano**: tocás el La central y completa los Hz solo; avisa antes de subir mucho la afinación.
- **Lupa (±10 cents) y estroboscopio** para el ajuste fino.
- **Aviso de tecla equivocada o armónico** cuando lo que suena no es la nota pedida.
- **Indicador de lectura estable**: te dice cuándo la aguja se quedó quieta.
- **Intervalos de control**: batido esperado de 3ras, 4tas, 5tas, 6tas y 8vas contra las notas ya afinadas.
- **Modo tira de fieltro**: primero todas las cuerdas libres y después los unísonos, como los afinadores.
- **Informe de afinación**: gráfico de desvíos sobre el teclado y exportar a CSV.
- **Referencia con sonido de piano** (se escucha hasta en los graves con parlantes de notebook), atajo `R`.
- **85 u 88 teclas**: muchos pianos viejos terminan en La; la app saltea las teclas que el tuyo no tiene.
- **No se pierde nada**: cada afinación se guarda con su piano. Solo termina cuando no falta ninguna nota (o cuando tocás *Terminar afinación*), *Empezar de nuevo* conserva la anterior y cualquier afinación se puede reabrir desde *Afinaciones guardadas*.
- **Captura por golpe en los agudos**: las notas agudas cortas se miden por golpe (sin el ruido del martillo, con la mediana de todo el golpe) y el resultado queda fijo hasta el próximo golpe, con los últimos 5 golpes a la vista. Avanza sola con 2 golpes buenos; en los unísonos agudos avisa si hay batido.
- **Volver a afinar el mismo piano**: la app recuerda el La de la última vez y te deja subirlo a esa tonalidad, ir primero a la mitad (si es mucho) o afinarlo donde está hoy.
- Interfaz en **español e inglés**.

## Consejos de micrófono (notebooks)

Desactivá las "mejoras de audio" y la cancelación de ruido (configuración de sonido de Windows, Lenovo Vantage, software de AMD/NVIDIA) y poné el volumen del micrófono alto. Esos filtros se comen los agudos cortos.

## Compilar desde el código

Necesitás [Rust](https://rustup.rs).

```
cargo build --release
```

El ejecutable queda en `target/release/PianoTunerFree.exe`.

## Créditos

El motor de detección de tono, inharmonicidad y orden de afinación viene de [pianito](https://github.com/4esv/pianito) de Axel Stevens (MIT). Este proyecto le suma la interfaz gráfica, las cuerdas por piano, el detector de batido, la medición del piano y más.

## Licencia

MIT. Ver [LICENSE](LICENSE).

---

**Palabras clave:** afinador de piano, afinar piano, app para afinar piano, afinador de piano gratis, programa para afinar piano, afinador de piano para PC, afinador de piano Windows, afinador cromático, afinación de piano, unísonos, batido, inharmonicidad, estiramiento, curva de Railsback, La 417 Hz, La 440, diapasón, temperamento, afinar piano con micrófono, afinador open source, software libre.
