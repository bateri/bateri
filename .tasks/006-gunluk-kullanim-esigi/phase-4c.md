# Phase 4c — Yerel yedeği `LANG=en_US.UTF-8`

## Özet

Sistem dil + bölge çiftinin yereli kurulu değilse kabuğa `LC_CTYPE=UTF-8`
yerine `LANG=en_US.UTF-8` verilir.

_Requirements: R4.4_

---

## Neden bu phase var

phase-4b'nin `/code-review`'u WAIVE önerisi (2) olarak buldu
(`phase-4b.md` → Uygulama Notları): düşüş kolunun `LC_CTYPE=UTF-8`'i Linux'ta
yerel adı değil ve macOS `ssh_config`'i `SendEnv LANG LC_*` ile onu uzak
makineye taşıyor → orada `setlocale` uyarısı. Bu makine tam o kolda (`en` +
`TR`, `en_TR.UTF-8` yok) ve kullanıcının bugünkü terminali çocuğa
`LANG=en_US.UTF-8` veriyor. Kullanıcı 2026-09-15'te yedeği `LANG=en_US.UTF-8`
seçti (`discussion.md` → Karar 6 eki, son madde).

Sıra: kullanıcının phase-3/3b/4/4b göz kontrollerinden **sonra**, phase-5
ölçümünden **önce** (R5.1: ölçüm son koda alınır). Göz kontrolleri bu phase
yüzünden tekrarlanmaz: değişen yalnız düşüş kolunun değişkeni, UTF-8 girişi
iki hâlde de çalışır.

---

## 1. Düşüş kolu

`crates/bt-shell/src/child.rs` → yerel kararı (`locale_env` ve çevresi).

Karar sırası:

1. Ortamda boş olmayan `LC_ALL`/`LC_CTYPE`/`LANG` varsa → hiçbir şey (değişmez).
2. `{dil}_{bölge}.UTF-8` kuruluysa → `LANG={dil}_{bölge}.UTF-8` (değişmez).
3. Değilse → **`LANG=en_US.UTF-8`** (yeni). macOS'ta her zaman kurulu;
   Linux sunucuların çoğunda da var. `en_US.UTF-8` de kurulu değilse
   (beklenmez) → `LC_CTYPE=UTF-8` son çare olarak kalabilir; tutup
   tutmadığını gerekçesiyle yaz.

Neden `LANG` ve neden `en_US`: kullanıcının bugünkü terminaliyle aynı değer;
`LANG` en zayıf değişken, rc dosyası üstüne yazabilir. Doc'taki "bilinen
bedel" (SSH uyarısı) cümlesi bu yeni hâle göre düzelir; alacritty/iTerm2'nin
`LC_CTYPE=UTF-8` düşüşünden bilerek ayrıldığımızı ve nedenini yaz.

`CLAUDE.md`'deki çocuk ortamı maddesi düşüş değerini sayıyorsa aynı commit'te
düzelir.

---

## Uygulama Notları

## Yayın Etkisi

---

## Checklist

- [ ] Düşüş kolu `LANG=en_US.UTF-8`; ilk iki kol değişmedi
- [ ] Doc'lar ve `CLAUDE.md` yeni düşüşü söylüyor (SSH gerekçesi dahil)
- [ ] Test: kurulu olmayan dil + bölge → `LANG=en_US.UTF-8` (önce eski beklentiyle kırmızı gör)
- [ ] Test: ortamda yerel varsa ve kurulu çift varsa davranış değişmedi (mevcut sınamalar yeşil)
- [ ] Paketli açılış yoklaması: bu makinede (`en` + `TR`) çocuk `LANG=en_US.UTF-8` görüyor
- [ ] `[elle]` yok — phase-4b'nin göz kontrolü kapsıyor (UTF-8 girişi iki hâlde de çalışır)
- [ ] Doğrulama geçti (`proje.md` → Doğrulama; `make duman` **zorunlu**; `make kur` koşar)
- [ ] `/simplify` çalıştırıldı, bulgular uygulandı
- [ ] `/code-review` çalıştırıldı, bulgular giderildi
- [ ] `/audit` çalıştırıldı, bulgular giderildi
- [ ] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
