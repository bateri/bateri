# Phase 5 — Reduce Motion

## Özet

Sistemin Hareketi Azalt ayarı ve `[motion] reduce_motion` anahtarı her
animasyonu 90 ms'lik belirmeye indirir; ikisi de canlı izlenir, süreli koşu
ikisini de görmez.

_Requirements: R6, R5 (reduce_motion yarısı)_

## Değişiklikler

- **`crates/bt-core/src/settings.rs`** — `[motion] reduce_motion`, üç değerli
  dizgi: `"system"` (varsayılan), `"on"`, `"off"`. `bool` değil, çünkü
  "sistemi izle" en olası seçim ve `bool`'da onu ifade etmenin tek yolu
  anahtarı **silmek** olurdu — bu dosyada anahtar silinmiyor.
- **`crates/bt-shell/src/app.rs`** — `"system"` iken
  `NSWorkspace::accessibilityDisplayShouldReduceMotion` okunur ve
  `NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification` ile canlı
  izlenir (emsali `apply_appearance`'ın açık/koyu izlemesi; bildirim
  `NSWorkspace`'in kendi merkezinden gelir). Karar **çözülmüş bir değer**
  olarak `bt-gpu`'ya iner: `bt-gpu` AppKit görmez (katman tablosu).
  `Inputs`'un doc'undaki "dört ayrı koşul" listesi beşinciyi kazanır: süreli
  koşu sistem ayarını okumaz, yoksa duman satırı ölçen makinenin
  erişilebilirlik ayarına bağlanırdı.
- **`crates/bt-gpu/src/motion.rs`** — indirgeme tek yerde: açıkken stil ne
  olursa olsun animasyon 90 ms'lik bir **belirme**dir; imleç yeni konumda
  görünür, eski konumda iz bırakmaz (çapraz solma değil — ikinci bir
  dikdörtgen ve alfa karışımı, görünmeyen bir kazanç için). Durma koşulu
  aynı disiplinde: süre dolunca yerleşir.
- **`crates/bt-gpu/src/frame.rs` + `shaders/`** — imleç dikdörtgeni belirme
  boyunca **alfa** taşır. `cell_bg` pipeline'ında harmanlama açık değilse bu
  phase onu açar (ya da imleci kendi harmanlı koluna alır); aynı alfa
  `cell`'in imleç uniform'una da geçer ki blok altındaki metin rengi
  dikdörtgenle birlikte belirsin — yoksa harf, henüz görünmeyen bir bloğun
  rengine boyanır.
- **`docs/AYARLAR.md`** — `### [motion]` bölümüne ikinci anahtar; sistem
  ayarının nereden okunduğu ve önceliği.

## Kabul

- Sistem ayarı açıkken (System Settings ▸ Accessibility ▸ Display ▸ Reduce
  Motion) imleç kaymaz, yeni konumda belirir; ayarı koşu sırasında açıp
  kapatmak pencereyi yeniden başlatmadan etkiler.
- `reduce_motion = "off"` sistem açıkken de kaymayı korur; `"on"` sistem
  kapalıyken de belirmeye indirir.
- Tanınmayan değer yalnız kendi anahtarını varsayılanda bırakır ve tanı
  gösterir.
- `make duman` etkilenmez ve makinenin erişilebilirlik ayarından bağımsızdır.

## Yayın Etkisi

**ayar şeması** — yeni anahtar `[motion] reduce_motion`, varsayılan
`"system"`; `docs/AYARLAR.md` aynı commit'te. **shader** — imleç alfası
`.metal` tarafına dokunuyorsa `make shader` koşar ve uniform düzeni alan alan
doğrulanır. terminfo yok · tema yok · shell entegrasyonu yok · app bundle yok ·
yeni bağımlılık yok.

`CLAUDE.md`'nin "`reduce_motion` ve sistemin Reduce Motion ayarı her
animasyonu 90 ms'lik solmaya indirir" cümlesi bu commit'le artık koda karşılık
geliyor; 90 ms **seçilmiş** bir sayıdır ve doc'u bunu söyler.

## Checklist

- [ ] `settings.rs`: `reduce_motion`, üç değer, varsayılan `"system"`
- [ ] `app.rs`: `NSWorkspace` okuması + bildirim gözlemcisi + hermetiklik
      (`Inputs` doc'u)
- [ ] `motion.rs`: 90 ms belirme, tek yerde indirgeme, durma koşulu
- [ ] İmleç alfası (`frame.rs` + gerekiyorsa shader/harmanlama)
- [ ] `docs/AYARLAR.md`
- [ ] Test: üç değerin ayrıştırılması; indirgemenin animasyonu 90 ms'de
      yerleştirmesi; hermetik koşunun sistem ayarını görmemesi
- [ ] Doğrulama geçti (`make hepsi` + `make duman`, shader değiştiyse
      `make shader`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
- [ ] Yayın etkisi yazıldı
