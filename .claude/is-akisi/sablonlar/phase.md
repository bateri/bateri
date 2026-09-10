# Phase {N} — {Başlık}

## Özet

{Bu fazda ne yapılacak — tek cümle}

_Requirements: R1, R2.1_

---

## 1. {Değişiklik Başlığı}

`crates/{crate}/src/{dosya}.rs`

{Açıklama + kod örneği. Shader değişiyorsa `.metal` ve Rust `#[repr(C)]`
karşılığı yan yana verilir.}

---

## Uygulama Notları

{Kod yazılırken ilk varsayımdan SAPAN her şey: API davranışı, ölçüm sonucu,
beklenmeyen etkileşim, bir TUI'nin gönderdiği beklenmedik dizi. Bu bölüm
teslim.md'nin ve sonraki okuyucunun referansıdır — çelişki çıkarsa Yayın
Etkisi'ne değil BURAYA güvenilir.}

## Yayın Etkisi

{Bu phase'in kullanıcı makinesindeki duruma ve belgelere etkisi — yoksa "yok"
yaz. Aranacak başlıklar `.claude/is-akisi/proje.md` → "Yayın etkisi"
bölümündedir (shader, terminfo, ayar şeması, tema biçimi, shell üçlüsü, app
bundle, ölçüm bekleyen iddia, belge, yeni bağımlılık).}

---

## Checklist

<!-- [x] yapıldı · [~] waive/atlandı (yanına gerekçe) · [ ] yapılmadı.
     Kutular silinmez — koşmayan kapının kutusu da dosyada kalır. -->

- [ ] {Yapılacak iş}
- [ ] Test: {test senaryosu}
- [ ] Doğrulama geçti (`proje.md` → Doğrulama; bu depoda `make hepsi` + koşullu komutlar)
- [ ] `/simplify` çalıştırıldı, bulgular uygulandı
- [ ] `/code-review` çalıştırıldı, bulgular giderildi
- [ ] `/audit` çalıştırıldı, bulgular giderildi
- [ ] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
