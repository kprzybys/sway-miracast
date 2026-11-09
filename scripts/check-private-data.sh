#!/usr/bin/env bash
# Blokuje commit, gdy dodawane wiersze albo opis commita pasują do prywatnych wzorców (MAC-i i nazwy sieci
# z miejsca testów, nazwy urządzeń). Repo ma być publiczne, więc lista leży POZA repo:
# SWAY_MIRACAST_PRIVATE_PATTERNS albo ~/.config/sway-miracast/private-patterns (wiersz = ERE, # = komentarz).
# Bez listy nic nie sprawdza — współtwórcy bez niej nie są blokowani.
# Użycie: check-private-data.sh            — zmiany w indeksie (pre-commit)
#         check-private-data.sh <plik-msg> — opis commita (commit-msg)
set -euo pipefail

list=${SWAY_MIRACAST_PRIVATE_PATTERNS:-$HOME/.config/sway-miracast/private-patterns}
[[ -r $list ]] || exit 0
patterns=$(grep -vE '^[[:space:]]*(#|$)' "$list" || true)
[[ -n $patterns ]] || exit 0

if [[ $# -gt 0 ]]; then
    what="opis commita"
    content=$(grep -v '^#' "$1")
else
    what="dodawane wiersze"
    # tylko dodawane wiersze, każdy poprzedzony nazwą pliku i tabulatorem
    content=$(git diff --cached --no-color --unified=0 \
        | awk '/^\+\+\+ /{file=substr($0, 7); next} /^\+/{print file "\t" substr($0, 2)}')
fi

if hits=$(grep -iE -f <(printf '%s\n' "$patterns") <<<"$content"); then
    echo "Dane prywatne ($what) — repo ma być publiczne. Zamień je na zastępniki (np. <TV-MAC>, <SSID>):" >&2
    # bez treści dopasowania: komunikat sam nie powtarza danych
    if [[ $# -gt 0 ]]; then
        echo "  opis commita" >&2
    else
        cut -f1 <<<"$hits" | sort | uniq -c | sed 's/^ */  /' >&2
    fi
    exit 1
fi
