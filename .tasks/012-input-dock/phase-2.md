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
- Ayar şeması, tema, shader, terminfo: yok. Yeni bağımlılık: yok.

## Checklist

- [ ] `add-zle-hook-widget line-pre-redraw` + `line-finish`; `zle -N` yok
- [ ] Beş değişken bildiriliyor; `region_highlight`'ın biçimi kanalda sabit
- [ ] Idempotan nöbet; geç kayıt olan eklentinin düşürme riski **adıyla** yazılı
- [ ] `line-finish` durumu boşaltıyor
- [ ] Test: `bateri.zsh`'in yeni kolları (bugün betiği koşturan sınama yok —
      bu phase en az bir tane getiriyor)
- [ ] Gerçek zsh oturumunda gözle: iki eklenti kurulu, ikisi de yaşıyor
- [ ] Doğrulama geçti (`make hepsi` + `make kur`)
- [ ] Yayın etkisi yazıldı ("ölçüm bekliyor + araç da borç" dahil)
