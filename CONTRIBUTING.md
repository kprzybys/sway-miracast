# Contributing to sway-miracast

## In English (short)

- Build: `make build` (release binaries in `target/release`). Rust from [rustup](https://rustup.rs); distribution
  packages are usually too old for gstreamer-rs.
- Test and lint: `make check` (rustfmt, clippy with warnings as errors, pre-commit on the whole repo, unit tests;
  no network and no TV needed).
- Hooks: `make hooks` installs pre-commit (via `uvx`). The hooks need `shellcheck` and `ruff` (fetched by
  pre-commit) and [gitleaks](https://github.com/gitleaks/gitleaks) on your `PATH` (`language: system`).
  The `no-private-data` hook only checks something when you have your own pattern list
  (`SWAY_MIRACAST_PRIVATE_PATTERNS` or `~/.config/sway-miracast/private-patterns`); without it, it passes.
- A bug fix or feature comes with a test when the logic can be separated (parsers, TS mux, PTS/PCR, padding).
  TV behaviour is checked by measurement; record new TV quirks as a profile in `sinks/` (template in `sinks/README.md`).
- Commit messages in English or Polish: a short title and a "what and why" body, with numbers from measurements.
- No MAC addresses, network or device names from your test site in code, docs or commit messages; use placeholders
  such as `<TV-MAC>`, `<SSID>`, `<TV name>`.

The rest of this document is in Polish.

## Jak pracujemy nad sway-miracast (po polsku)

Własne źródło Miracast (Wi-Fi Display) dla Sway/wlroots: niskie opóźnienie i wiele godzin pracy bez obsługi.
Metoda pracy z odbiornikami (specyfikacja jako minimum, zachowania TV w `sinks/`): [docs/METODA.md](docs/METODA.md).

### Przygotowanie

```bash
make hooks   # pre-commit: formatowanie, clippy, shellcheck, ruff, gitleaks (musi być w PATH), blokada danych prywatnych
make check   # lint + testy (bez sieci i bez TV)
make build   # target/release
```

Rust z rustup (`~/.cargo/bin`), nie z pakietów systemu (za stary dla gstreamer-rs).

### Zasady narzędzi

- **Jeden program, jedna rzecz.** `wfd-source` steruje (P2P, RTSP, nadzór), `wfd-media` przechwytuje i koduje,
  `wfd-tx` pakuje i wysyła, `wfd-proto` tylko parsuje (bez I/O). Granice to procesy i FIFO, nie wspólny stan.
- **Log to JSON w jednej linii na stderr** (`{"event":...}`), trafia do journald. Każde zdarzenie, które zmienia
  zachowanie strumienia (restart, korekta czasu, wyrzucenie danych), ma swój wpis.
- **Ścieżka czasu krytycznego bez niespodzianek:** zegar monotoniczny, każde czekanie z niezerowym limitem,
  bez blokujących wywołań w pętli `wfd-tx`, bez alokacji w gorącej ścieżce, gdy da się ich uniknąć.
- **Praca bez obsługi:** awaria jednego elementu kończy się jego restartem albo czystym końcem sesji
  (powrót do czekania na TV), nigdy zawieszeniem. `panic`/`unwrap` tylko tam, gdzie błąd jest niemożliwy
  albo przy starcie programu.
- **`unsafe`** tylko przy FFI (libc, Wayland, GStreamer) i zawsze z komentarzem `// SAFETY:`.

### Zmiany

- Każda poprawka błędu i każda nowa funkcja przychodzi z testem, który bez niej nie przechodzi, o ile logika
  da się wydzielić (parsery, mux TS, rachunek PTS/PCR, dopełnianie). Zachowania TV sprawdzamy pomiarem.
- **Najpierw odtworzyć, potem poprawiać.** Zmiana, która ma poprawić opóźnienie albo dźwięk, ma pomiar przed
  i po (`tools/audiolat.py`, `tools/tonegap.py`), wynik w opisie commita.
- Działającą usługę zmieniamy świadomie: zabicie `wfd-media` jest bezpieczne (restart kodera bez zrywania sesji),
  zabicie `wfd-tx` albo restart usługi kończy sesję i wymaga ponownego Smart View.
- `make check` musi przejść. Wyjątek od reguły lintera — w kodzie, z uzasadnieniem w komentarzu.
- Opisy commitów po angielsku albo po polsku: krótki tytuł i opis „co i dlaczego”, z liczbami z pomiarów.

### Dane prywatne

Repo ma być publiczne. Żadnych MAC-ów, BSSID-ów, nazw sieci ani urządzeń z miejsca testów w kodzie, dokumentach,
danych i opisach commitów — zastępniki: `<TV-MAC>`, `<SSID>`, `<AP-BSSID>`, `<TV name>`. Hook `no-private-data` sprawdza
dodawane wiersze i opis commita według listy wzorców spoza repo (`~/.config/sway-miracast/private-patterns`); bez tej listy niczego nie blokuje.
Nagrań z kamery i mikrofonu nie commitujemy (osoby w kadrze, rozmowy); zostają tylko liczby.
Surowe logi z pomiarów (adresy, nazwy sąsiednich sieci) przed dodaniem trzeba zamaskować albo streścić.
