# Phase 2 — ZLE tesisatı: beş değişken kanaldan akıyor

## Özet

Kabuk betiği her satır yeniden çiziminde ZLE'nin görüntü durumunu — beş
değişken — phase-1'in kanalından bildirsin.

_Requirements: R1.1, R6.2_

## Değişiklikler

- **`assets/shell/zsh/bateri.zsh`** — bugün `zle` hiç geçmiyor; tesisat
  sıfırdan.
  - **`add-zle-hook-widget line-pre-redraw`** ve `line-finish`. **`zle -N`
    kullanılmaz:** zsh-syntax-highlighting ve zsh-autosuggestions aynı widget'ı
    istiyor ve `zle -N` onları düşürür (011 ölçtü).
  - **Beş değişken:** `PREDISPLAY`, `BUFFER`, `POSTDISPLAY`, `region_highlight`,
    `CURSOR`. Dördü metin/dizi, biri sayı. `region_highlight` bir **dizi**
    (`start end spec` üçlüleri) ve biçimi kanalda sabitlenir.
  - **Idempotan nöbet.** `PS1`'in üç ekinin nöbeti içerme sınamasıyla
    (`bateri.zsh`'in kurulum bloğu); ZLE kancası için karşılığı yazılır —
    `.zshrc`'mizden **sonra** kanca kuran bir eklenti (zsh-defer, p10k
    instant-prompt sonu) bizimkini düşürebilir ve bu bugün **belirtisiz**.
  - `line-finish` durumu kapatır: Enter'dan sonra dock boş kalmalı, son
    `BUFFER` asılı durmamalı.
  - **Kullanıcının rc dosyasına yazılmaz** — kapısı `make denetim`.
- **`crates/bt-core/src/shell.rs`** — gelen beş alan `DockState`'e çözülür;
  `region_highlight`'ın zsh biçimi burada **sınırda çözülmüş** renk aralığına
  çevrilir (`CLAUDE.md` → karar burada, boyama orada).

Bu phase'te de **hiçbir şey çizilmiyor**: `dock_state()` doluyor, tüketicisi
phase-3.

## Kabul

- Gerçek bir zsh oturumunda yazarken `dock_state()` doluyor ve `line-finish`
  onu boşaltıyor.
- zsh-syntax-highlighting ve zsh-autosuggestions **kurulu** bir oturumda ikisi
  de çalışmaya devam ediyor (widget düşmemiş) ve katkıları
  `region_highlight`/`POSTDISPLAY` üzerinden kanalda görünüyor.
- Uzun bir satır phase-1'in sınırını aşınca **görünür** sonuç dönüyor.
- `make kur` yeşil: betik pakete kopyalanıp `cmp` ile denetleniyor.

## Yayın Etkisi

- **Shell entegrasyonu:** yalnız zsh. bash (`--rcfile`) ve fish
  (`vendor_conf.d`) **kapsam dışı** ve gerekçesi planda (dock yalnız
  entegrasyonlu zsh'te). Kullanıcı rc dosyasına dokunulmuyor.
- **`make kur` zorunlu** (`assets/shell/*` değişti).
- **Ölçüm bekliyor + araç da borç:** tuş başına O(n) bayt (beş değişken,
  base64). Gidiş-dönüş yankınınkiyle aynı olduğu için net etki bir kayıp
  olmayabilir — ama ölçülmedi **ve** ölçecek kanca yok
  (`BT_INPUT_LATENCY_SAMPLES`). `/measure` bugün kapatamaz.
- **Bilinen sınır:** p10k **instant prompt** kancalarımızdan önce koşuyor
  (`.zshrc`'miz kullanıcının dosyasını top-level `source` ediyor).
- **Tel biçimi büyüdü:** üçüncü işlem `o` (kabuğun uzunluk kapısı →
  `DockFault::Overflow`). Biçimin iki ucu da bu sette doğuyor, yani geriye
  dönük bir okuma borcu yok; `CLAUDE.md` güncellendi.
- Ayar şeması, tema, shader, terminfo: yok. Yeni bağımlılık: yok.

## Uygulama Notları

- **`shell.rs` kalemi phase-1'de kapanmıştı.** `region_highlight`'ın zsh
  biçimini çözen yol (`P` öneki, `memo=`, kırpma) orada yazıldı; bu phase
  Rust tarafına yalnız **üçüncü işlemi** (`o`) ekledi, gerekçesi aşağıda.
- **`line-init` planda yoktu, eklendi.** `line-pre-redraw` yalnız satır
  *değişince* koşuyor: gerçek bir PTY oturumunda gözlendi, ilk ayna ancak ilk
  tuş vuruşunda geliyordu, boş prompt'ta hiç gelmiyordu. Onsuz dock prompt anında
  ölü kalır, ilk harfte birden belirirdi — ve phase-4'ün bastırma kararı tam
  da "prompt anında ayna canlı mı" sorusuna dayanıyor.
- **Ayrı bir idempotan nöbet yazılmadı: kancanın kendisinde var.**
  `add-zle-hook-widget` aynı widget'ı iki kez eklemiyor; iki kez kaydedip
  `add-zle-hook-widget -L line-pre-redraw` listesinin tek satır kaldığı
  görüldü. `PS1`'in eklerinde elle yazdığımız nöbetin karşılığı hazır, ikincisi
  ölü kod olurdu. Düşürme riski betikte **adıyla** duruyor (zsh-defer bir
  çizim geç; `zle -N zle-line-pre-redraw` diyen eklenti dağıtıcıyı büsbütün
  ezer).
- **Kodlayıcıda sessiz bir bozulma yakalandı.** Bayt değerini doğrudan
  `##${bytes[i]}` ile okumak aritmetiğin kaçış dizisi yorumuna giriyor ve ters
  bölü baytı 92 yerine 32 okunuyordu — komut satırında sık geçen bir bayt.
  Değer önce skalere alınıyor (`x=$bytes[i]`, sonra `#x`); sınamanın gövdesinde
  ters bölü bilerek var.
- **Döngüsüz kodlayıcı denendi ve elendi.** İki toplu ikame (bayt → 8 bit,
  6 bit → harf; ikisi de zsh'in C tarafında) tipik satırda azıcık ucuz ama
  uzun girdide **kareselleşiyor** — yoklandı, döngülü hâlin birkaç katı.
  Sadelik ve maliyet aynı yönü gösterdi.
- **Kabuk tarafında bir uzunluk kapısı planda yoktu, eklendi**
  (`__bateri_dock_limit`, 4096 karakter). Kodlama saf zsh ve maliyeti girdinin
  uzunluğuyla **doğrusal**, üstelik tuş başına ödeniyor; kapı olmasaydı
  yapıştırılmış bir blok terminalin `DOCK_PAYLOAD_LIMIT`'te zaten reddedeceği
  bir yükü kodlamak için harcanırdı — bedeli öder, karşılığını alamazdık. Sayı
  yeni değil: 64 KiB'lik terminal sınırı zaten "4096 karakter × 4 bayt ×
  4/3"ten türemişti, bu onun karakter cinsinden hâli. İki ucun sonucu **aynı**
  olmak zorunda olduğu için tele üçüncü bir işlem girdi: `o` →
  `DockFault::Overflow`. Aynı sinyali `Malformed`'a bindirmek `DockFault`'un
  iki varyantının tek sebebini ("sınırı büyütmem mi gerek") silerdi.
  Kapı **dört gövdeyi birden** ölçüyor: `region_highlight` toplamın içinde
  değil, yanında ayrı bir terim — sözdizimi vurgusu jeton başına bir kayıt
  bırakıyor ve kısa bir metnin yanında kendi başına sınırı aşabiliyor.
  `DOCK_PAYLOAD_LIMIT`'in türetmesi de onu zaten ayrı sayıyordu.
- **Tuş başına iki ayna gelebiliyor** (phase-3'ün bilmesi gereken bir gözlem):
  eklentili oturumda önce `POSTDISPLAY`/`region_highlight`'sız bir `u`, hemen
  ardından eksiksiz olanı basılıyor — eklentiler kendi kancalarında satırı
  yeniden çiziyor. Dock ilkini hevesle çizerse bir karelik renksiz/önerisiz
  yanıp sönme görünür.
- **Eklenti doğrulaması gerçek eklentilerle yapıldı.** İkisi de bu makinede
  kurulu değildi; depoları geçici bir dizine klonlanıp sarmalayıcının altında
  koşturuldu. İkisi de yaşıyor ve katkıları telde görünüyor: `POSTDISPLAY`
  autosuggestions'ın önerisini (`rhaba dünya`), `region_highlight`
  syntax-highlighting'in kayıtlarını (`0 4 fg=green memo=zsh-syntax-highlighting`)
  taşıyor. Kullanıcının kendi kurulumu değişmedi.

## Checklist

- [x] `add-zle-hook-widget line-pre-redraw` + `line-finish`; `zle -N` yok —
      ayrıca `line-init` (gerekçe Uygulama Notları'nda)
- [x] Beş değişken bildiriliyor; `region_highlight`'ın biçimi kanalda sabit
- [x] Idempotan nöbet; geç kayıt olan eklentinin düşürme riski **adıyla**
      yazılı — nöbet `add-zle-hook-widget`'ın kendisinde, doğrulandı
- [x] `line-finish` durumu boşaltıyor
- [x] Test: `bateri.zsh`'in yeni kolları — betiği gerçekten koşturan dört
      sınama (kodlayıcı ↔ çözücü turu, üç dolgu artığı ve UTF-8, `line-finish`,
      uzunluk kapısı) + gerçek ZLE'li bir PTY oturumu (`session.rs`)
- [x] Gerçek zsh oturumunda gözle: iki eklenti kurulu, ikisi de yaşıyor
- [x] Doğrulama geçti (`make hepsi` + `make kur` + `make test-yaris`; okuma
      yolu değiştiği için yarış da koştu, iki profilde de yeşil — **yeni
      paylaşılan durum yok**, yani riskli phase tetiklenmedi ve kendi
      `/code-review`'ı gerekmedi)
- [x] Yayın etkisi yazıldı ("ölçüm bekliyor + araç da borç" dahil)
