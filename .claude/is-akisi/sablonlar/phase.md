# Phase {N} — {Başlık}

<!-- Phase dosyası kodun KILAVUZUDUR, kopyası değil: ne değişecek, neden,
     hangi sınır korunacak, nasıl doğrulanacak. Kod örneği yazılmaz; bir imza,
     tip adı ya da `#[repr(C)]` alan sırası sözleşmeyse tek satırla adı geçer.
     Gerekçe tartışması discussion.md'de — burada tekrarlanmaz. -->

## Özet

{Bu fazda ne yapılacak — tek cümle}

<!-- Yalnız çok phase'li sette; tek phase'li sette bu satırı sil. -->
_Requirements: R1, R2.1_

## Değişiklikler

- **`crates/{crate}/src/{dosya}.rs`** — {ne değişir; korunacak sınır ya da
  sözleşme varsa o}

## Kabul

- {Doğrulanabilir sonuç: hangi sınama, hangi jeton, hangi davranış}

## Uygulama Notları

<!-- Kodlanırken doldurulur. YALNIZ ilk varsayımdan sapan şey, madde başına
     bir-iki satır. Sapma yoksa bölümü sil. -->

## Checklist

<!-- [x] yapıldı · [~] waive/atlandı (yanına gerekçe) · [ ] yapılmadı.
     Kutular silinmez. Riskli phase kutusu yalnız proje.md → Kalite kapısı →
     "Riskli phase" koşulu tetiklenirse kalır; tetiklenmiyorsa ya da bu SON
     phase'se (set kapısı onu kapsıyor) üretirken sil. -->

- [ ] {Yapılacak iş}
- [ ] Test: {test senaryosu}
- [ ] Doğrulama geçti (`make hepsi` + koşullu komutlar)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
