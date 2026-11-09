# Profile odbiorników (sinks)

Jeden plik na model: `<producent>-<platforma>-<model>.md`. Zasady: `docs/METODA.md`.

| Profil | Rozpoznanie (RTSP User-Agent / P2P) | Stan |
|---|---|---|
| [samsung-tizen65-hg50bu800](samsung-tizen65-hg50bu800.md) | `SVDTZ6_5/SEC-UIBC3`; P2P ModelNumber `QAQ80`, WFD dev-info 0x0111 | używany, 1080p60 ~90 ms |

## Szablon

```
# <Producent> <model> (<platforma>, firmware jeśli znany)
Rozpoznanie: User-Agent …; P2P: nazwa/model/pri_dev_type; WFD IE …
Możliwości: odnośnik do sondy (data/probe/…): kodeki, tryby, audio, UIBC, rozszerzenia.
Ustawienia źródła: tryb, bitrate, audio w M4, parametry dodatkowe.

## Zachowania
### Q<n>: <krótka nazwa>
- Obserwacja:
- Dowód: (log/pomiar, ścieżka, data)
- Odtworzenie: (jak wywołać celowo)
- Specyfikacja: (co mówi / „brak”)
- Obejście: (gdzie w kodzie) — Status: potwierdzone | hipoteza | obejście w kodzie | otwarte
```
