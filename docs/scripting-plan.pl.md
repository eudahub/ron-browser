# Plan: JavaScript i WebAssembly

Przeglądarka nie uruchamia jeszcze żadnych skryptów. Tu opisane jest, jak ma je uruchamiać, nie będąc
mniej bezpieczna niż Firefox, i w jakiej kolejności. Nic z tego nie jest jeszcze zbudowane. Pomiary są w
[`bench/hash`](../bench/hash/README.md), a to, co zmierzono tu przy okazji, uruchomiono raz, ręcznie.

## Co musi być spełnione

- Skrypt nie może zmienić zachowania przeglądarki. Wstecz, pasek adresu i menu są poza jego zasięgiem, a
  Wstecz wraca przed każde przekierowanie, także wykonane przez skrypt.
- Skrypt dociera do sieci, dysku i użytkownika tylko przez prośbę do przeglądarki, która może odmówić.
- Strona bez skryptów nic nie kosztuje: nie ma na nią procesu ani pamięci.
- Skrypt, który działa za długo albo zajmuje za dużo pamięci, zostaje przerwany, a strona zostaje.

## Warstwy zabezpieczeń

Atak musi przejść przez wszystkie po kolei. Zawiedzenie jednej z nich to błąd, ale jeszcze nie włamanie.

1. **Co do zasady bez kompilacji w trakcie działania strony.** Kompilator, który zamienia to, co napisała
   strona, na kod maszynowy w trakcie działania, to częste źródło groźnych błędów w przeglądarkach.
   Kosztuje to szybkość, więc to jedyna warstwa z wyjątkami: etap 4 kompiluje proste, często używane
   procedury, tylko dla stron, które tego potrzebują, i w osobnym procesie. Cała reszta obywa się bez
   tego, dzięki czemu system może też zakazać takiemu procesowi tworzenia nowego kodu wykonywalnego.
2. **Silnik zamknięty w WebAssembly.** Błąd w silniku zostaje w jego własnej pamięci.
3. **Osobny proces z ograniczeniami systemu** (seccomp, namespaces), bez sieci i dysku. To zabezpiecza też
   na wypadek ucieczki z samej piaskownicy WebAssembly, co się Wasmtime zdarzało: CVE-2026-34987 i
   CVE-2026-34971.
4. **Bardzo wąski interfejs do DOM.** To najbardziej ryzykowne miejsce, bo tam łatwo o błędy logiczne, więc
   każde wywołanie jest sprawdzane po stronie Rusta.
5. **Skrypty działają tylko tam, gdzie zezwoli użytkownik.** Domyślnie są wyłączone.
   - Biała lista nie jest listą pojedynczych adresów jak w NoScript. Zezwolenie na bieżącą stronę, np. banku,
     obejmuje skrypty, do których się w tej chwili odwołuje, i tylko na tej witrynie. Nowe z innych serwerów
     czekają na decyzję, bo dozwolony skrypt może doładować sieć reklamową.
   - Czarna lista ma pierwszeństwo przed białą. Strona może mieć użyteczne skrypty obok reklam, także na tym
     samym serwerze, więc użytkownik klika część okna z reklamą, a to zapisuje stojący za nią plik, katalog
     albo serwer. Liczy się najdokładniejszy wpis, bez parametrów adresu, bo reklamy je zmieniają. Działa to,
     bo nasz własny interfejs do DOM wie, który skrypt utworzył dany element.
   - Dla ważnych stron można zachować skrót skryptu i pytać ponownie, gdy skrypt się zmieni.

   Czarna lista obowiązuje też obrazki, ramki i arkusze stylów, więc blokuje reklamy bez żadnych skryptów i może
   powstać dużo wcześniej niż one.

## Skąd ta kolejność

Czas tej samej pracy, na jednym wątku (szczegóły w `bench/hash`):

| | SHA-256, 10000 razy | Argon2id, 64 MB, 3 przebiegi |
|---|---|---|
| WebAssembly w Chrome | 3 ms | 103 ms |
| WebAssembly w Wasmtime | ~2 ms | 130 ms |
| WebAssembly w wasmi | ~26 ms | 1,0 s |
| JavaScript w Chrome | 5 ms | 0,76 s |
| QuickJS w Wasmtime | 171 ms | ~42 s |
| Boa | 728 ms | ~211 s |
| QuickJS w wasmi | 1,9 s | ~589 s |

- WebAssembly w piaskownicy jest blisko tego, co robią przeglądarki, więc to, co strony piszą w nim dla
  szybkości, pozostaje szybkie. Idzie jako pierwsze i jest prostsze niż JavaScript.
- Silnik w piaskownicy jest przy tej pracy, najtrudniejszej dla niego, 40 do 55 razy wolniejszy niż Chrome.
  Zwykłe `onclick` tego nie zauważy, a Argon2 napisany w JavaScripcie już tak.
- JavaScript na początku nie jest zamieniany na WebAssembly. Zamieniany jest silnik, raz, przy budowaniu
  przeglądarki, a skrypt to dla niego tylko dane. Konwersja niewiele daje bez znajomości typów, znanych
  dopiero w trakcie działania, a LLVM jest o wiele za duży i za wolny, by go używać przy oglądaniu strony.
  Etap 5 wraca do tego dla funkcji, w których typy da się poznać.
- WebAssembly nie działa samo. Liczy tylko na własnej pamięci i wywołuje to, co dostało, a wczytuje je i
  wywołuje JavaScript: forum może wystawić Argon2 jako plik WebAssembly z krótkim skryptem wokół. Silnik
  jest więc potrzebny także dla WebAssembly, ale ma być poprawny, a nie szybki, bo ciężka praca odbywa się
  gdzie indziej.

## Etapy

### 1. WebAssembly ze stron w wasmi

wasmi to interpreter, więc nic nie jest kompilowane. W osobnym procesie z warstwą 3, z limitami pamięci i
kroków, które wasmi potrafi liczyć. Bez wątków i pamięci współdzielonej, bo dają dokładne liczniki czasu,
potrzebne w atakach na procesor. Na tym etapie dostaje tylko to, co przekaże mu JavaScript, więc przydaje
się dopiero razem z etapem 2; proces piaskownicy i limity powstają i są sprawdzane jako pierwsze.

### 2. JavaScript w silniku skompilowanym z wyprzedzeniem

Silnik to nasz własny, niezmienny kod, więc Wasmtime może go skompilować przy budowaniu przeglądarki.
Przeglądarka wczytuje gotowy plik i nie zawiera żadnego kompilatora, więc nic, co robi strona, nie wpływa na
kod maszynowy. Sprawdzone ręcznie na QuickJS: kompilacja trwa 0,1 s i daje plik 3 MB, który działa równie
szybko i startuje w mniej niż 10 ms, w 21 MB.

Wasmtime ma osobne funkcje `runtime` i `cranelift`, ale przeglądarkę zbudowaną tylko z `runtime` trzeba
jeszcze wypróbować, podobnie jak zakaz nowej pamięci wykonywalnej przy wczytywaniu takiego pliku.

Do wyboru na podstawie pomiarów, na skryptach zapisanych przez `--save-page`:

| | Za | Przeciw |
|---|---|---|
| QuickJS | Tu około 6 razy szybszy od Boa. Mały. | Rdzeń w C, choć w piaskownicy. |
| Boa | Czysty Rust, około 96% testów Test262. | Najwolniejszy z porównywanych silników. |

Wybór może pozostać otwarty, jeśli silnik będzie za jednym interfejsem.

Na początek wyłączone: `SharedArrayBuffer` i `Atomics`, dokładne liczniki czasu oraz `eval`, chyba że okaże
się potrzebny stronom. Bezpieczeństwo daje głównie ograniczenie tego, co skrypty mogą zrobić, a nie tego,
co mogą napisać.

Te same pomiary policzą, jakich konstrukcji i interfejsów strony naprawdę używają, co rozstrzygnie, co dać
najpierw. Zapisana strona nie zawsze jest pełna: skrypty wymienione tylko w innych skryptach nie są jeszcze
znajdowane.

### 3. Interfejsy w Ruście, poza silnikiem

`crypto.subtle` i podobne działają natywnie, więc haszowanie hasła nie zależy od szybkości silnika. Tu też
powstaje droga do DOM, wywołanie po wywołaniu, każde ze swoimi sprawdzeniami, jak w warstwie 4. Skrypty
dostają uchwyty do węzłów, a nic o pamięci.

### 4. Kompilacja często używanych prostych procedur WebAssembly

Najpierw wszystko w wasmi, a potem procedury, które wykonują się wiele razy i zostały uznane za proste, są
kompilowane do kodu maszynowego, zamiast całego modułu:

- Prosta to tylko arytmetyka, pętle i dostęp do pamięci ze sprawdzeniem zakresu, bez wywołań przeglądarki,
  tablic funkcji i referencji. Takie są funkcje mieszające Argon2 i SHA.
- WebAssembly i tak ogranicza jego walidacja, więc „prosta” dotyczy tego, jak mało ma do zrobienia kompilator,
  bo to w nim może tkwić błąd.

Do rozwiązania wtedy:

- Skompilowany kod musi działać na tej samej pamięci co wasmi. Wasmtime tego nie zrobi, więc trzeba użyć
  bezpośrednio Cranelifta albo małego własnego kompilatora.
- Procedura wywołana raz, z długą pętlą, przełączy się dopiero przy następnym wywołaniu. Przełączanie w trakcie
  pętli jest dużo trudniejsze i zostaje na później.
- Taki proces nie może zakazać tworzenia kodu wykonywalnego, więc działa to tylko dla stron, które tego
  potrzebują, w osobnym procesie, po przekroczeniu limitu kroków albo za zgodą użytkownika.

### 5. Często używane funkcje JavaScriptu jako WebAssembly

Funkcje, które wykonują się wiele razy i działają tylko na liczbach i tablicach typowanych, są tłumaczone z
kodu bajtowego silnika na WebAssembly, ze sprawdzaniem typów i powrotem do interpretera, gdy się nie zgadzają.
Daje to najwięcej i jest najtrudniejsze, więc idzie na końcu i tylko jeśli etap 3 okaże się niewystarczający.

## Założenie

Porządna strona z ciężkimi rachunkami, jak forum z Argon2, wystawia je jako WebAssembly, a JavaScript wykonuje
powolną pracę: reaguje na kliknięcia i wywołuje inne rzeczy. Silnik ma więc być poprawny, a nie szybki, a etap 5
nie jest planowany, chyba że pomiary pokażą inaczej.

Dotyczy to rachunków. Inaczej jest ze stronami na dużych frameworkach: wykonują dużo JavaScriptu już przy starcie,
i to nie na liczbach, lecz na obiektach i napisach, a w tym etap 5 by nie pomógł. Ile to trwa, trzeba zmierzyć jako
pierwsze na etapie 2.

## Otwarte pytania

- Który silnik, po pomiarach (etap 2).
- Jak strona w jednym procesie ma rozmawiać z DOM w drugim dostatecznie szybko. Każde wywołanie przechodzi
  między procesami, więc może trzeba je łączyć w partie.
- Jak zrobić ograniczenia systemowe poza Linuksem.
- Czy uruchamiać strony we własnym interpreterze Wasmtime, którego nie mierzono.
