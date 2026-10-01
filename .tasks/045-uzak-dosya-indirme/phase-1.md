# Phase 1 — Saf zemin: uzak hit, ayar anahtarları, kurallar

## Özet

Bütün kararların I/O'suz yarısı: uzak hit'in kapısı, `[remote]`'un yeni
anahtarları ve uzak dosyanın kuralları (`bt-shell-common`), davranış henüz
değişmeden.

_Requirements: R1, R3, R5.3, R6, R8_

## Değişiklikler

- **`crates/bt-core/src/session.rs`** — `link_allowed` uzakta düz metin yolu
  reddetmez; hit uzak olduğunu taşır (`LinkHit`'te ya da `LinkKind::Path`'in
  yanında, imza kodda seçilir). `file://` uzakta kapalı kalır (Karar 13).
  `dock_link_at` aynı kapıdan.
- **`crates/bt-shell-macos/src/hyperlink.rs`** — uzak hit bu phase'de
  **yerel olarak doğrulanmaz**, bağlantı yokmuş gibi düşer (yerel `stat`
  uzak yolu yanlışlıkla bulabilir). Phase 3'te yerini yardımcı oturum alır.
- **`crates/bt-core/src/settings.rs`** — `[remote]`'a R8'in sekiz anahtarı:
  boyutlar `"100MB"`/`"2GB"` biçimli (birim tablosu tek yerde), süre
  `preview_keep` (`"launch"`, `"1d"`, `"7d"`, `"30d"`), çakışma enum'u
  `NAMES` deseniyle, yollar `~` açılımlı dizgi; varsayılan, şablon satırı,
  tanı, `SettingsEdit` + `place`/`value`, `Changes::remote` genişler.
  Bilinmeyen anahtarı koruyan round-trip sınaması.
- **`crates/bt-core/src/dock.rs`** — `ButtonLabel::ShowFiles` metni "Show
  transfers (N)" (sığma kuralı aynı).
- **`crates/bt-shell-common/src/remote_files.rs`** (yeni) —
  - yardımcı oturumun protokolü: tek satırlık istek kodlaması, `sh` döngü
    betiği, cevap ayrıştırma (var mı, tür, boyut, mtime, `x` biti; klasörde
    dosya sayısı ve toplam boyut); adlar indeksle, `upload`'un `sq`/reddetme
    kuralı (ters bölü, kontrol karakteri);
  - indirme betiği (`tar c -C dir ./ad`, `cd` başarısızlığının çıkış kodu);
  - `scp_path(target, path)`: argv'den `-p`/`Port=` portu, çevrilemeyen
    seçenekte yalnız `host:/yol`;
  - uzak açma politikası: dosya → önizleme (düz metin mi, varsayılan mı:
    044'ün `DOCUMENT_TYPES` kararını içerik cevabıyla birleştirir), klasör →
    yok;
  - önbellek yol eşlemesi `{dir}/{host}/{uzak mutlak yol}` (`..`, mutlak
    olmayan ve kaçış denemesi reddedilir);
  - temizlik planlayıcısı: girdiler (yol, boyut, mtime, son açılış, bateri'nin
    yazdığı boyut/mtime, tetik: açılış/günlük/Clear Now) → silinecekler ve
    Downloads'a taşınacaklar.
- **`crates/bt-shell-common/src/upload.rs`** — `sq`, `is_safe`,
  `remote_command` `remote_files`'ın kullanacağı kadar görünür olur (taşıma
  değil paylaşma; tek kopya).
- **`docs/AYARLAR.md`** — `[remote]` tablosuna sekiz anahtar, şablon kopyası,
  OSC 7'yi sunucuda açan tek satırlık rc örneği (Karar 2).

## Kabul

- `make check` yeşil; `make linux` yeşil (`bt-core`, `bt-shell-common`).
- Sınamalar: uzak hit `Some` + uzak işaretli, `file://` uzakta `None`;
  sekiz anahtarın ayrıştırma/tanı/varsayılan/round-trip'i; protokol ve
  ayrıştırıcı (bozuk cevap panik değil hata); scp yolu (`-p 2222`,
  `ssh://h:2222`, `-J` → yalnız `host:/yol`); önbellek eşlemesinin kaçış
  reddi; temizlik planlayıcısının üç tetiği ve farklılaşmış kopya kuralı.
- Uygulamada davranış değişmez (uzakta yine bağlantı yok).

## Checklist

- [x] `link_allowed` + uzak işareti; hyperlink'te uzak hit düşer
- [x] `[remote]` anahtarları, şablon, tanı, `SettingsEdit`, `Changes`
- [x] "Show transfers (N)"
- [x] `remote_files.rs`: protokol, indirme betiği, scp yolu, politika, önbellek yolu, temizlik planı
- [x] `docs/AYARLAR.md`
- [x] Test: yukarıdaki Kabul listesi
- [x] Doğrulama geçti (`make check`, `make linux`)

## Uygulama Notları

- Uzak işaret `LinkHit::remote` alanı (yalnız `Path`'te `true`, `choose` taşır); `hyperlink.rs` iki `link_at` çağrısında `!hit.remote` süzgeciyle düşürür, `same_link` alanı da karşılaştırır.
- `Changes::remote` tek alan kaldı: `remote_hosts` **ya da** `remote_files` değişince `true`; tek tüketicinin (`set_host_marks`) yeniden gönderimi zararsız.
- `preview_keep` süre ayrıştırıcısı değil `NAMES` enum'u (`PreviewKeep`, dört değer); boyut birimsiz tam sayıyı da reddeder. Varsayılanlar tasarım tuvalinin Settings ekranından (`"7d"`, `~/Library/Caches/bateri/Previews`); çakışmanın yazımı `"keep_both"`.
- Yardımcı oturumun isteği `eval` edilen tek satır `sh` (`bt_stat`/`bt_count` + `sq`'lu yollar); klasör boyutu `du` değil `find … -exec ls -ln` toplamı (tar'ın akıttığı bayt). Dizin içeriği yalnız `Ask::Count`'ta — hover'da ağaç yürünmez.
- Temizlikte indekste kaydı olmayan kopya hiçbir tetikte silinmez/taşınmaz (bozuk indeks → hiçbir şey), ama boyut toplamına girer.
- `dock.rs` sınamaları "Show transfers" etiketinin dört sütunu için genişletildi (60 → 64 sütun, eşikler +4).
