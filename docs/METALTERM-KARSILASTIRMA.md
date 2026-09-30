# Metalterm karşılaştırması (26 Eylül 2026)

`pioner92/metalterm-site` sürüm notlarının (0.1.3 → 0.2.0, 20 Ağu – 22 Eyl
2026) `bateri`'nin 036'daki hâliyle yan yana okunması. **Tarihli bir
kayıttır** ve `docs/ARASTIRMA.md` gibi güncellenmez; Metalterm'in binary
envanteri oradadır. Bir maddeyi işe çevirmek yol haritasının işidir
(`docs/YOL-HARITASI.md`), bu dosya yalnız neyin eksik olduğunu söyler.

Kaynak yalnız sürüm notları. Notlarda geçmeyen bir şey Metalterm'de yok
demek değildir. Metalterm'in verdiği sayılar onların kendi iddiasıdır ve
burada ölçülmedi.

## İkisinde de olanlar

| Özellik | Metalterm | bateri |
|---|---|---|
| Input Dock ve kapatma seçeneği | 0.1.6 | 012; `[shell] integration = "blocks"` |
| Dock'ta fare ve Shift+ok ile seçim, seçimi silme, tıklanan yere caret | 0.1.4, 0.1.11 | 031 |
| Dock'ta yazma ve silme efektleri; imleç hareketini kapatma | 0.1.9, 0.1.11 | 030; `cursor_motion = "snap"` |
| Komut satırının sağında süre, koşarken canlı | 0.1.8 (eşiği ayarlanabilir) | 013 (eşik bir saniye, sabit) |
| Komut şeridi ve koşan / başarılı / hatalı renk | 0.1.7 | 010 |
| Kapatırken koşan işi sorma (never / running / always) | 0.1.8 | 028; `confirm_close` |
| Finder'dan bırakılan dosyanın kaçırılmış yolu | 0.1.3 (resim yapıştırma da) | 018 (yalnız dosya damlası) |
| Ölü tuşlar ve ABD dışı klavye düzenleri | 0.1.3 (Option tuşu başına kip) | 018 (Option politikası sabit) |
| Fare raporu; Shift ile uygulama içinde seçim | 0.1.9 | 020, 031 |
| Grapheme dizileri (ZWJ, VS16, bayrak) | 0.1.9 | 035 |
| ⌘F arama, ⌘E seçimle arama | 0.1.4, 0.1.12 | 033 (kümenin ikinci kod noktası aranmıyor, bilinen sınır) |
| ⌘A her şeyi seç | 0.1.4 | var |
| Satır aralığı | 1.0–1.4, varsayılan 1.2 | 1.0–2.0, varsayılan 1.0 |
| Pürüzsüz kaydırma ve kapatılması; boşta sıfır kare | 0.1.8, 0.1.9 | 027; `smooth_scroll` |
| Sekmeler | 0.2.0'da özel sekme çubuğuna geçti | 026, macOS'un kendi sekmeleri |
| Close Other Tabs | 0.2.0 | 028 |

## Metalterm'de olup bizde olmayanlar

Sıra kullanıcıya etkisine göre kabaca verildi.

1. **Bölmeler.** ⌘D / ⇧⌘D ile iki eksende bölme; klavyeyle gezinme, boyut
   değiştirme, eşitleme ve büyütme (0.1.6). Yol haritasında "bölme" olarak
   var ve bedeli yazılı.
2. **⌘-tık ile bağlantılar.** URL'ler, çıplak `ls` adları, boşluklu yollar,
   OSC 8 ve satır sonunda sarılmış bağlantılar (0.1.8, 0.1.12). Yol
   haritasında "tıklanabilir bağlantılar" olarak var.
3. **Satır içi programların dock'un yerini alması** (0.1.3). Klavyeyi
   kendisi alan program koşarken dock gizleniyor ve yeri programa veriliyor.
   Metalterm bunu ad listesiyle değil "davranışından" yapıyor ve yöntemi
   yazmamış. Bizde dock yalnız alternatif ekranda kalkıyor. 26 Eylül'de
   ölçüldü (sahte terminalde ilk altı saniyedeki mod dizileri):

   | Program | Alternatif ekran | bateri'de dock |
   |---|---|---|
   | `claude` | evet (`?1049h`) | kalkıyor (kullanıcı gördü) |
   | `codex` | hayır | kalıyor (kullanıcı gördü) |
   | `python3` | hayır | kalır (aynı yol, pencerede görülmedi) |
   | `node` | hayır | kalır (aynı yol, pencerede görülmedi) |

   Zor tarafı ayırıcı sinyal: `make` de koşan bir komut ve orada barın
   kalması kullanıcının kararı (yol haritası → "uzun yerel komutta giriş
   satırını gizleme"). İki aday var, ikisi de henüz sınanmadı. Birincisi
   bracketed paste (`?2004h`): python3 ve codex açtı, node ilk koşuda
   açmadı. İkincisi PTY'nin satır kipinden çıkması.
4. **Materyal yüzeyler.** 21 materyal, grain/sheen ve Material/Classic
   sekmeleri (0.1.6). Bizde yok. Yol haritasında numarasız "materyal yüzey"
   maddesi olarak duruyor.
5. **Arka plan bulanıklığı ve pencere saydamlığı** (0.1.8). Reduce
   Transparency'yi izliyor.
6. **Zengin durum çubuğu.** Git sayaçları, Python venv adı, yol ve sayaç
   animasyonları (0.1.7, 0.1.9). Bizde bağlam satırında yalnız `yol | dal`
   var.
7. **Sekme durum rozetleri** (0.2.0). İş koşarken nabız atıyor, başarıyı
   kısa süre onaylıyor, arka plandaki hatayı tutuyor; açıkça bildirilen
   ilerleme için alt çubuk var.
8. **Sekme yönetimi** (0.2.0). Move Tab to New Window; sekmeyi pencereler
   arasında oturumu koruyarak sürükleme; tek oturumda sekme çubuğunu gizleme;
   sekme sağ tık menüsü. Bizim sekmeler macOS'un olduğu için sürükleme belki
   hazır geliyor, ama doğrulanmadı.
9. **Pencere geri yükleme** (0.1.8, 0.1.12). Spaces, bölme düzeni, dizinler
   ve son görünen metin.
10. **Tema içe aktarma** (0.1.12). Ghostty ve Kitty biçimi, ayarlarda
    düzenlenebilir şablon. Bizde elle yazılan `themes/*.toml` var.
11. **Başka kabuklar.** fish, Nushell ve bash (0.1.11). Bizde yalnız zsh.
12. **Küçük macOS entegrasyonları.**
    - Finder Services: "New Metalterm Tab/Window Here" (0.1.7)
    - Dock menüsünde New Window/Tab (0.1.7)
    - Terminal yüzeyinde sağ tık menüsü (0.1.9)
    - Ayarlarda Reset all (0.1.3)
    - Help menüsünde issue ve sürüm notu bağlantıları (0.1.4)
    - ⌘J ile seçime dönme (0.1.4)
    - Izgarada Shift+ok ile seçim; Esc, yazma ve gezinme tuşlarıyla seçimi
      temizleme (0.1.8, 0.1.9)
13. **SGR-pixel fare raporu (1016)** (0.1.9). Bizde 1006, 1005 ve X10 var;
    1016 kodda adıyla kapsam dışı.
14. **Dağıtım.** Sparkle ile otomatik güncelleme, universal binary (Intel +
    Apple silicon), Homebrew cask, notarization. Bizde Developer ID imzası,
    notarization (`make package`) ve Sparkle (`make ship`, besleme GitHub Releases) var;
    paket yalnız Apple silicon ve Homebrew cask yok.
15. **Satır içi alt komut önerisi** (0.1.7). git, paket, container, bulut ve
    derleme araçlarının alt komutları geçmişte olmasa da öneriliyor. Bizde
    öneri yalnız kullanıcının zsh eklentisinin `POSTDISPLAY`'inden geliyor.

## Bizde olup notlarda geçmeyenler

Notlarda geçmemesi, Metalterm'de olmadığı anlamına gelmez.

- Dock'ta çok satırlı giriş ve salt okunur `PREBUFFER` (032)
- Tamamlama listesi kapanınca ekranın eski hâline dönmesi (017)
- Kutu, blok, Braille ve terminal grafik karakterlerinin fonttan bağımsız
  çizimi (021)
- Terminal tarafında ⌘K / ⌥⌘K ve Paste Escaped Text (034)
- ssh ve mosh'ta `⇄ host` göstergesi (036)

## Karşılaştırılamayanlar

- **Ayrıştırma hızı.** Metalterm yoğun çıktıda 183–186 MB/s ve 2.5 kat az
  ayırma bildiriyor (0.1.4). Bizde bu hiç ölçülmedi; bench seti borç.
- **Hücre boyu.** Metalterm %25 küçülme bildiriyor; `docs/ARASTIRMA.md`'ye
  göre hücreleri 20 bayt. Bizim grid hücremiz 24 bayt (alacritty `Cell`).

## Ayrışan tasarım kararı

Metalterm 0.2.0'da macOS'un sekmelerini bırakıp kendi sekme çubuğunu yazdı.
Sebep olarak anında açılmayı, kapanma ve sıralama animasyonlarını, rozetleri
ve pencereler arası canlı sürüklemeyi sayıyor. 026 bunun tersini seçti.
Rozet ya da ilerleme çubuğu istenirse o karar yeniden açılmalı.
