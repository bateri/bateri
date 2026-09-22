---
name: plan-review
description: Bir planlama setinin yaklaşımını koda dönüşmeden önce bağımsız jüri paneliyle sınar — aşırı mühendislik var mı, daha basit yol var mı, mevcut mimariyle kavga ediyor mu. Kullanıcı bir planı gözden geçirtmek, "bu tasarım temiz mi" diye sormak ya da /rfc akışında seçenekleri sunmadan önce vetlemek istediğinde kullanılır.
allowed-tools: Read, Write, Edit, Glob, Grep, Agent
---

Kullanıcı bir planlama setinin (`.tasks/{NNN}-{slug}/`) seçilen yaklaşımını
**koda dönüşmeden önce** bağımsız gözlerle sınamak istiyor.

`/code-review` kodu inceler; bu skill **tasarımı** inceler — kötü bir plan
`/implement` tarafından sadakatle uygulanır ve tasarım hatası en ucuz burada
yakalanır.

Set düzeni için `.claude/is-akisi/duzen.md`, projenin mimari sözleşmesi için
`.claude/is-akisi/proje.md` ve `CLAUDE.md`.

## Girdi

`$ARGUMENTS` = iş klasörü adı (`003-ek-fiil-indeksi`) ya da yalnız numarası /
slug'ı — `.tasks/` altında eşleştir. `context.md` + `discussion.md` (varsa) +
`plan.md` (taslak dahil) oku, önerilen yaklaşımı ve alternatifleri çıkar.
Hiçbiri yoksa dur: "önce `/rfc` ile context/discussion üret".

## 1. Jüri paneli (3 mercek, paralel)

Her jüri **karşılaştırmalı** çalışır: önerilen yaklaşımı hem discussion'daki
alternatiflere hem kendi üreteceği "daha basit yol"a kıyaslar. Jürilere
context+discussion+plan içeriğini, `CLAUDE.md`'yi ve repo erişimini ver;
aramayı repoyla sınırla (host geneli tarama yasak).

| Mercek | Soru | Çıktı |
|---|---|---|
| **Sadelik / YAGNI** | Aşırı mühendislik nerede? Aynı hedefe daha az parça/katman/kavramla gidilir mi? | "En basit çalışan tasarım" sketch'i + öneriyle kıyas |
| **Codebase-fit** | Plan mevcut örüntüyle kavga ediyor mu? Katman yönü korunuyor mu, var olan mekanizma yeniden mi icat ediliyor? | Gerçek dosya yollarıyla kanıtlı itirazlar |
| **İşletme** | Türetilmiş dosya/ölçüm/belge yükü, geri alınabilirlik, kısmi uygulama kırılganlığı, sessizce ne bozulur? | Yük envanteri + basitleştirme fırsatları |

Mercek başına **tek ses karar verdiği için model tavizi yok**: her jüri
`opus` ile koşar (`model: 'opus'`).

Her jüriden dönecek biçim:

- **Verdict**: `TEMİZ` (uygula) / `SORUNLU` (itirazlar giderilmeli) /
  `KIRMIZI` (yaklaşım değişmeli)
- En güçlü **1–3 itiraz** — kanıtlı (dosya yolu / somut senaryo); zayıf ya da
  spekülatif itiraz getirme
- Varsa **daha temiz alternatif** — 5–10 satır sketch, tam tasarım değil

**Sınır:** Panel tasarımı sorgular, kapsamı **genişletmez**. "Şunu da eklesek"
türü öneri kapsam dışıdır, taşıma.

**İkinci sınır — jüri ürün kararına yetkili değil.** Mercekler "koda uyar mı",
"daha basit var mı", "yükü ne" diye sorar; hiçbiri "kullanıcı ne görür" diye
sormaz. Bir bulgunun **çözümü** kullanıcının gördüğünü değiştiriyorsa o artık
bir ürün sorusudur ve adım 4'te kullanıcıya **sorulur**, sentezde karara
bağlanmaz. Ölçülmüş örnek: 023'te Codebase-fit doğru bir kod kısıtı buldu
(dock'un sütunu karakter indeksinden geliyor) ve ondan "dock'ta geniş bayrağı
hiç kurulmasın" sonucunu çıkardı; sentez onu değişmez diye plana yazdı ve
sonuç, kullanıcının yazdığı emojinin dock'ta kutu çıkması oldu. Kısıtın iki
çözümü vardı ve jüri yalnız kodu koruyanı görüyordu.

Bu projeye özgü mercek notları:

- Yeni bir crate bağımlılığı öneren plan → otomatik itiraz konusu; taban
  liste `CLAUDE.md`'dedir ve dışına çıkmak mimari karardır. Özellikle
  "kendi VT ayrıştırıcımızı yazalım" → `alacritty_terminal` neden yetmiyor?
- Renderer'a (`bt-gpu`) terminal semantiği koyan ya da `bt-core`'a platform
  kütüphanesi sokan plan → itiraz: katman yönü ve platformsuzluk `CLAUDE.md`'de.
- Her frame'de CPU tarafında hesap yapan bir efekt önerisi → itiraz: durma
  koşulu nerede, shader parametresi olarak GPU'ya taşınamaz mı, boşta sıfır
  kare korunuyor mu?
- Hücre yapısına alan ekleyen plan → itiraz: yan tablo neden olmuyor?
- `TERM` adını değiştiren ya da terminfo dağıtan plan → itiraz: SSH'daki
  uzak makinede geri düşüş tasarlanmış mı?
- Shell entegrasyonuna dokunan plan → üç kabuğu (zsh, bash, fish) birden
  kapsamalı; kullanıcı rc dosyasına yazan her yol KIRMIZI.
- Ölçüm iddiası taşıyan plan ("120 fps tutar", "gecikme düşer") → itiraz:
  ölçüm tahmin edilmez, `/measure` ile gösterilir; plan iddiayı hiç yazmaz.

## 2. Sentez (ana döngüde — devredilmez)

İtirazları tek tek değerlendir: hangileri **kabul** (plana işlenecek), hangileri
**red** (gerekçesiyle). Jüri gürültü üretebilir; yalnız tasarımı gerçekten
değiştirecek itirazı taşı.

## 3. Muhakeme kaydı

`discussion.md`'ye (yoksa `plan.md`'ye) `## Muhakeme` bölümünü yaz — biçim
`.claude/is-akisi/sablonlar/discussion.md` içindedir. Bu bölüm aynı zamanda
"bu plan muhakeme gördü" işaretidir; `/implement` ön uçuşta buna bakar.

## 4. Kullanıcıya özet

Verdiktler + kendi önerin (planla devam / planı değiştir / seçenek değiştir).
**Karar kullanıcınındır**; kabul edilen değişiklikler plan.md'ye ya da
discussion.md'ye onayıyla işlenir.

## Ne zaman koşulur

- `/rfc` akışında (adım 6): **yalnız** birden çok yaklaşım varsa ve seçim
  pahalı bir sınıfa dokunuyorsa (liste orada). Varsayılan kapalı; gerekçesi
  de orada.
- Bağımsız: herhangi bir eski planın üstüne de koşturulabilir ("bu plan temiz
  mi?") — bu kullanımda bulgular doğrudan kullanıcıya sunulur.
