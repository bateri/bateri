CARGO ?= cargo

# Prerequisite sırası yalnız seri make'te garantidir; -j altında "en ucuz kapı
# önce" ve "sürüm başta" sözü bozulur.
.NOTPARALLEL:
.PHONY: hepsi fmt denetim clippy test shader duman terminfo test-yaris tarama kur paket yukle yayin yayinla gonder yayin-kapisi sparkle dmgbuild linux

# Definition of done. Homebrew rustc pin'li değil (rust-toolchain.toml bilinçli
# olarak yok): bir `brew upgrade` sonrası gelen clippy kırmızısını kod
# kırmızısından ayırmak için sürüm başta basılır.
hepsi:
	@rustc --version
	@$(MAKE) --no-print-directory fmt denetim clippy test

fmt:
	$(CARGO) fmt --all -- --check

# Proje kurallarının mekanik yarısı (`/audit`'ten taşındı): ajansız ve
# saniyeler içinde, her `make hepsi`'de. Yargı isteyen mercekler `/audit`'te
# kalır ve set sonunda bir kez koşar (`.claude/is-akisi/proje.md`).
# - Katman: `cargo tree` Cargo.toml'daki sözleşmeyi, grep kaynağa sızan
#   çağrıyı görür (`cfg` arkasındaki kullanım ağaçta görünmeyebilir). Kaynak
#   grep'i yorum satırlarını düşürür: bt-core'un başlık yorumu "objc2 yok" der.
#   bt-atlas'ta `objc2-core-*` serbest, yalnız `objc2` çekirdeği yasak.
# - Panik: bt-core'un sınama dışı kodunda unwrap/expect/panic!/unreachable!
#   yok; bilinçli olanın satırında `// audit: {neden}` durur. Tarama satır
#   başındaki ilk `#[cfg(test)]`'te durur, çünkü sınama modülü dosyanın sonunda.
# - Shell: `assets/shell/` altında kullanıcı rc dosyasına yazan satır yok.
#   Liste zsh'in BEŞ dosyasını da taşıyor: sarmalayıcının yönlendirdiği
#   dosyalar tam olarak onlar ve kapı, adı listede olmayan dosya için soruyu
#   hiç sormuyor. `.zlogout` bizde bir dosya olmasa da listede — kapı bizim
#   dizinimizi değil kullanıcının dosyalarını koruyor.
# - bt-gpu platform kütüphanesi görmez (040): doğrudan bağımlılığında (dev
#   dahil, `--depth 1`) ve kaynağında objc2/dispatch2/block2/metal yok. GPU'ya
#   wgpu üzerinden ulaşılıyor; wgpu'nun Metal arka ucunun **dolaylı** çektikleri
#   bu kontrolün konusu değil — sözleşme crate'in kendi kodu ve bildirimi,
#   arka ucun iç bağımlılıkları wgpu'nun işi. Katman ve vsync ritmi bt-shell'den
#   geliyor (`Surface::from_layer`, `Pacer`).
# - Bağımlılık DÜŞÜRMEZ, uyarır: bilinçli bir bağımlılık kararı da Cargo.lock'u
#   değiştirir; kararın kaydını `/audit` arar.
denetim:
	@fail=0; \
	if $(CARGO) tree -p bt-core -e normal | grep -E "objc2|core-text|core-graphics|metal"; then echo "denetim: bt-core platform kütüphanesine bağlanıyor"; fail=1; fi; \
	if $(CARGO) tree -p bt-atlas -e normal | grep -E "(^|[ ─])objc2 v"; then echo "denetim: bt-atlas objc2 çekirdeğine bağlanıyor"; fail=1; fi; \
	if $(CARGO) tree -p bt-gpu -e normal | grep -E "bt-shell"; then echo "denetim: bt-gpu yukarı, bt-shell'e bağlanıyor"; fail=1; fi; \
	if grep -rn "objc2\|core_text\|core_graphics" crates/bt-core/src | grep -v ":[[:space:]]*//"; then echo "denetim: bt-core kaynağında platform çağrısı var"; fail=1; fi; \
	if $(CARGO) tree -p bt-gpu -e normal,dev --depth 1 | grep -E "objc2|dispatch2|block2|metal"; then echo "denetim: bt-gpu platform kütüphanesine doğrudan bağlanıyor"; fail=1; fi; \
	if grep -rnE "objc2|dispatch2|block2|metal" crates/bt-gpu/src | grep -v ":[[:space:]]*//"; then echo "denetim: bt-gpu kaynağında platform çağrısı var"; fail=1; fi; \
	for f in crates/bt-core/src/*.rs; do \
		awk -v file="$$f" '/^[[:space:]]*#\[cfg\(test\)\]/{exit} /\.unwrap\(\)|\.expect\(|panic!|unreachable!/ && !/\/\/ audit: / && !/^[[:space:]]*\/\//{print file":"NR": "$$0; hit=1} END{exit hit}' "$$f" \
			|| { echo "denetim: bt-core'da gerekçesiz panik yolu ($$f)"; fail=1; }; \
	done; \
	if [ -d assets/shell ] && grep -rnE "(>>?|sed -i|tee).*(\.zshenv|\.zprofile|\.zshrc|\.zlogin|\.zlogout|\.bashrc|\.bash_profile|\.profile|config\.fish)" assets/shell; then echo "denetim: shell entegrasyonu kullanıcı rc dosyasına yazıyor"; fail=1; fi; \
	git diff --quiet HEAD -- Cargo.lock $$(git ls-files '*Cargo.toml') || echo "denetim: uyarı — Cargo.toml/Cargo.lock HEAD'den farklı; bağımlılık kararı kayıtlı mı?"; \
	if grep -rnEi "bateri|bt-(core|gpu|shell|atlas)|make [a-z]|cargo|crates/|assets/|metal|olcumler|yol-harita|arastirma|ayarlar\.md|zsh|dock|emoji|glyph|\bcrate|alacritty|terminfo|origin/main" .claude/skills .claude/is-akisi/duzen.md .claude/is-akisi/sablonlar; then echo "denetim: genel iş akışı dosyasında proje izi var — yeri .claude/is-akisi/proje.md"; fail=1; fi; \
	test $$fail -eq 0 && echo "denetim: temiz"

clippy:
	$(CARGO) clippy --workspace --all-targets -- -D warnings

test:
	$(CARGO) test --workspace

# Pencereyi açar, BT_RUN_SECONDS dolunca kare, hücre, glyph, kural çizgisi ve
# atlas yuvası sayısına bakar:
# kare=N hucre=K glif=G kural=R yuva=U/T yuva2=U/T yuk=smoke istek=I icerik=C \
#   hareket=M kayma=S sessiz=Sms kapanis=clean profil=debug ornek=off pipeline=ok
# İlk dördünden (kare, hucre, glif, kural) ya da `hareket`ten BİRİ 0 ise
# kırmızı; `yuva`, `yuva2`, `yuk`, `istek`, `kayma` ve `profil` kapı değil,
# sayaç ve etiket. `yuva` atlasın **maske** düzlemi, `yuva2` **renk** düzlemi
# (023): ikisi aynı yuva ızgarasını paylaşıyor, ayrı sayaçları var ve
# toplamları aynı. Reçete emoji basmadığı için `yuva2=0/T` bekleniyor;
# satırda olma sebebi tanı — göremediği bir düzlem 021'in Braille şekli olurdu
# (sıfır yuva harcayan, sessiz). `kayma` içeriğin ötelemesinin tanığı (011) ve reçete onu tetiklemediği
# için 0 bekleniyor; satırda olma sebebi tanı, `hareket` ile birlikte okununca
# hangi animatörün yerleşmediğini ayırt ediyor.
# `hareket` 008'de sayaçlıktan gerekliliğe geçti: duman reçetesinde bir
# imleç hareketi var (bt-core smoke_shell), yani 0 "animasyon yolu hiç koşmadı"
# demek. Gizli bağı da orada yazılı — gereklilik varsayılan imleç stilinin
# ANIMASYONLU olmasına dayanıyor.
# İKİNCİ ANİMASYON KAPISI JETONDA GÖRÜNMEZ: deadline'da yerleşmemiş bir
# animasyon varsa koşu kırmızı düşer (app.rs Verdict::MotionUnsettled) ve satır
# hiç basılmaz — karar `verdict`te, çünkü jeton satırı yalnız yeşil koşuda
# basılıyor. Hızdan bağımsız olması bütün değeri: `icerik` sınırı ancak
# yeterince hızlı bir sızıntıyı görüyor.
# ÜST SINIR `icerik`te, `kare`de DEĞİL (bkz. app.rs IDLE_FRAME_LIMIT): boşta
# sıfır kareyi bozan değişikliğin belirtisi eksik kare değil FAZLA karedir, ama
# meşru bir animasyon da `kare`yi şişirir — sınır bu yüzden çizilmeye KARAR
# VERİLEN içerik karesini sayıyor. `yuva` bir kapı değil sayaç — eşiği
# ölçülmedi, ölçülmemiş sayı kapıya yazılmaz.
# ALT SINIR `sessiz`te (son kareyle deadline arasındaki süre; hiç kare yoksa
# `none` ve o da kırmızı): 008 phase-6'da ÖLÇÜLDÜ ve kapıya bağlandı
# (app.rs QUIET_FLOOR; sayının ve türetmesinin sahibi docs/OLCUMLER.md).
# Kuralı ötekilerin TERSİ — sağlıklı koşuda büyük, sızıntıda küçük — ve kapının
# en duyarlı katı: periyodu tabandan kısa HER sızıntıyı görüyor, `icerik` sınırı
# ise ancak yeterince hızlı olanı. Sınırın altında kalacak kadar seyrek kare
# isteyen bir sızıntı bu kol olmadan YEŞİL geçiyordu (ölçülen kanıt o dosyada).
# Taban `BT_RUN_SECONDS`ın süresine ve smoke_shell'in uykusuna BAĞLI: süreyi
# kısaltan onu da yeniden türetmeli, yoksa kapı kod doğruyken düşer.
# `kapanis` KISMEN kapı: panik kolları kırmızı düşürür, kayıtlı borç olan iki
# kol düşürmez — bağlansaydı kapı bilinen bir borç yüzünden kırmızı düşerdi.
# `ornek=off` = ölçüm kapısı (BT_FRAME_STATS) kapalıydı; ölçüm jetonları o
# koşuda HİÇ basılmaz.
# Jetonların tam listesi ve sözleşmesi: app.rs Report::token_line; `kapanis`
# değerleri teardown_token'da, `insufficient` push_span'de.
# Başsız ortamda binary stdout'a "ATLANDI" basıp 78 ile çıkar; make bunu 2
# olarak döndürür — ayırt edici sinyal stdout metnidir, çıkış kodu değil.
# `cargo run` CARGO_TARGET_DIR'a saygı duyar ve çocuğun çıkış kodunu geçirir.
# `env -u`: kabukta ihraç edilmiş bir BT_SCROLL_TEST ya da BT_FRAME_STATS
# kapıyı SESSİZCE başka bir koşuya çevirirdi — `var_os` değere değil VARLIĞA
# bakıyor, yani `BT_SCROLL_TEST=` bile yükü seçer. O koşu `yuk=load` basıp
# exit 0 verir, `hucre`/`kural` yarısı ise hiç sınanmaz: kapı yeşil kalır ama
# iddia ettiğinden başka bir şeyi sınar. Kapı hermetik olmalı.
# Süreli koşuda pencere kayan seviyede açılıyor (`float_for_timed_run`): wgpu
# örtülü pencereye drawable vermiyor ve kapı başka bir uygulama öndeyken
# `kare=0` ile düşüyordu (040 phase-7).
duman:
	env -u BT_SCROLL_TEST -u BT_FRAME_STATS BT_RUN_SECONDS=3 $(CARGO) run -q -p bateri

# WGSL kanaryası: shader'lar `include_str!` ile gömülü ve derleme adımları yok,
# yani kanarya pipeline'ları kuran sınama — naga doğrulaması + Vulkan'ın
# immediate tabanıyla istenmiş device'ta pipeline kurulumu (040 Karar 9).
# Tek kanarya olduğu için "1 passed" aranıyor: sınamanın adı değişir ya da
# `cfg` arkasına düşerse cargo sıfır sınamayla 0 döner ve kapı sessizce
# yeşil kalırdı.
shader:
	@out=$$($(CARGO) test -p bt-gpu --lib -- --exact renderer::wgpu_tests::wgsl_pipelines_build 2>&1); st=$$?; \
	echo "$$out" | tail -3; \
	test $$st -eq 0 && echo "$$out" | grep -q "test result: ok. 1 passed" || { echo "shader: kanarya koşmadı ya da düştü"; exit 1; }

# Paylaşılan duruma (PTY okuyucu thread'i ↔ kare üreten taraf) dokunan
# değişikliklerde koşar. ThreadSanitizer nightly ister; araç zinciri pin'li
# değil (`rust-toolchain.toml` bilinçli yok, Homebrew rustc) ve nightly yok:
# yerine iki farklı zamanlama profili — önce yalnız race_* stresi,
# sonra ignore'lular DAHİL bütün takım tek thread'de. İkinci satır
# --include-ignored taşımasa `make test`in kopyası olurdu ve yarış
# sınamasını hiç koşmazdı. TSan nightly gelince BURAYA üçüncü satır olur.
test-yaris:
	$(CARGO) test --workspace -- --ignored race_
	$(CARGO) test --workspace -- --include-ignored --test-threads=1

# Yedek glyph kapısının envanteri (041): sembol ve emoji bloklarındaki her
# karakteri atlasın kendi kapısından geçirir ve 13/16pt × @1x/@2x için blok
# başına grupları (tabanda / yedekten sığdı / hiçbir fontta yok / kapıdan
# döndü), dönenlerin oran histogramını ve font başına dökümü basar. Oranların
# tanımı `crates/bt-atlas/src/census.rs` → `classify`. `make hepsi`'de YOK:
# sonucu makinede kurulu fontlara bağlı, kapı olamaz. `BT_SCAN_FONT=Aile`
# taban aileyi değiştirir. Release, çünkü on bine yakın karakter ölçülüyor.
tarama:
	$(CARGO) test -p bt-atlas --release -- --ignored census --nocapture

# Release derler ve `bateri.app`'i target/ altında kurar (/Applications'a
# DEĞİL). İçinde Sparkle (`Contents/Frameworks/`, aşağıda) ve imza var;
# notarization `paket`'in işi, çünkü Apple'a gidiyor ve dakikalar sürüyor.
# İmzasız paket de denetimden önce en az **ad-hoc** imzalanıyor (`codesign -s -`):
# linker'ın binary'ye koyduğu imza paketin kaynaklarını mühürlemiyor ve
# başka bir Mac'e zip'le giden kopyayı Gatekeeper "hasarlı" deyip çöpe
# atıyordu (kullanıcının arkadaşında görüldü). Geçerli ad-hoc imzayla aynı
# kopya "tanımlanmamış geliştirici" uyarısına iniyor ve Sistem Ayarları →
# Gizlilik ve Güvenlik → "Yine de Aç" ile açılıyor.
#
# İmza kimliği (`SIGN_ID`) **kendiliğinden seçiliyor**: anahtarlıkta geçerli
# bir "Developer ID Application" varsa o, yoksa "Apple Development", ikisi de
# yoksa ad-hoc (`-`). Hatırlanacak bir ayar olmasın diye; sertifikanın adı
# kişisel veri taşıdığı için (e-posta) depoya yazılmıyor, her koşuda
# anahtarlıktan okunuyor. Elle ezilir: `make kur SIGN_ID=-` ad-hoc'u,
# `SIGN_ID="ad"` belirli bir kimliği zorlar. Fark
# kimliğin **kalıcılığı**: ad-hoc imzanın tanımladığı gereksinim paketin
# kendi parmak izi ve her derlemede değişiyor, yani macOS her derlemeyi
# başka bir uygulama sayıyor ve verilen izinler (erişilebilirlik, tam disk
# erişimi) güncellemeden sonra yeniden soruluyor. Anahtarlıkta üretilmiş
# kendi sertifikan o gereksinimi sertifikaya bağlıyor ve derlemeler arasında
# sabit tutuyor. Başka bir Mac'te Gatekeeper açısından hiçbir şey değişmiyor
# — sertifikaya kimse güvenmiyor, uyarı aynı; onu yalnız Developer ID ile
# notarization kaldırır. Kimlik yoksa `codesign`'dan önce adıyla düşer.
#
# Kimlik ad-hoc değilse imza **hardened runtime**'la (`-o runtime`) atılıyor:
# notarization onu istiyor ve kütüphane doğrulaması paketteki Sparkle'ı
# yalnız aynı takımın imzasıyla yüklüyor. Developer ID'de ayrıca **güvenli
# zaman damgası** (`--timestamp`, Apple'ın sunucusu — `kur` o kimlikle ağ
# ister); notarization damgasız imzayı reddediyor. Apple Development ve
# ad-hoc damgasız kalıyor: notarize edilmeyecekler ve `make yukle` çevrimdışı
# da çalışmalı.
SIGN_OPTS = $(if $(filter -,$(SIGN_ID)),--timestamp=none,--options runtime $(if $(findstring Developer ID Application:,$(SIGN_ID)),--timestamp,--timestamp=none))
SIGN = codesign --force --sign '$(SIGN_ID)' $(SIGN_OPTS)
SIGN_ID ?= $(eval SIGN_ID := $$(shell ids=$$$$(security find-identity -v -p codesigning 2>/dev/null); \
	for kind in "Developer ID Application" "Apple Development"; do \
		n=$$$$(printf '%s\n' "$$$$ids" | sed -n "s/.*\"\($$$$kind: [^\"]*\)\".*/\1/p" | head -n 1); \
		[ -n "$$$$n" ] && { echo "$$$$n"; exit 0; }; \
	done; echo -))$(SIGN_ID)
#
# Paket önce `$(STAGE)`'de kurulur ve yalnız denetimden geçerse `$(APP)`'in
# yerine taşınır: yerinde kurulsaydı düşen bir koşu Dock'un gösterdiği yolda
# lisanssız ya da ikonsuz, açılabilir bir paket bırakırdı.
#
# Girdilerin İÇERİĞİNİ (`assets/bundle/`, `assets/shell/`) `make hepsi` içindeki
# `bundle_assets` sınar; buradaki denetim ÜRÜNÜ sınar: girdi yerinde durup
# pakete kopyalanmasa sınama yeşil kalırdı. Denetimin listesi kopya
# satırlarından bilerek ayrı yazılıyor — aynı değişkenden okusaydı kopyadan
# silinen dosya denetimden de silinirdi.
#
# `LSMinimumSystemVersion` binary'nin kendi `minos`'undan doldurulur, TOML
# ayrıştırılmaz: `.cargo/config.toml`'un `[env]`'i rustc'ye oradan ulaşıyor
# ama kabukta ihraç edilmiş bir `MACOSX_DEPLOYMENT_TARGET` onu eziyor. Plist
# binary'den okununca ikisi hiç ayrışamaz — ayrışsa LaunchServices uygulamayı
# açamayacağı bir sistemde açardı.
#
# URL şeması (`CFBundleURLTypes` → `bateri`, 038) da ürünün içinde aranıyor:
# `bateri://tab/<id>` LaunchServices'e yalnız paketin plist'inden kayıtlı ve
# şablondan düşen şema sessizce "open hiçbir şey açmıyor"a dönerdi.
#
# `X = $(eval X := $$(shell …))$(X)`: tembel VE bir kez. Düz `=` her açılışta
# komutu yeniden koşardı (`$(APP)` tarifte yirmiden fazla açılıyor), `:=` ise
# her make çağrısında — `hepsi` dahil. Hedef dizini `cargo metadata`'dan:
# `CARGO_TARGET_DIR` tek kaynak değil (`build.target-dir` ve
# `CARGO_BUILD_TARGET_DIR` de var) ve yanlış dizin bayat bir binary'yi
# sessizce paketlerdi. Kapsamadığı: `build.target` üçlüsü (`target/<üçlü>/`).
TARGET_DIR = $(eval TARGET_DIR := $$(shell $(CARGO) metadata --format-version 1 --no-deps | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p'))$(TARGET_DIR)
APP = $(TARGET_DIR)/release/bateri.app
STAGE = $(APP).partial
ICONSET = $(TARGET_DIR)/release/bateri.iconset
# Güncelleme: Sparkle 2 (`bt-shell::updater` onu çalışma zamanında yüklüyor).
# Framework depoya girmiyor; sürüm ve sha256 burada sabit, tarball ilk
# `kur`'da `$(SPARKLE_DIR)`'e iniyor ve özeti tutmayan indirme düşüyor.
# Yalnız `kur` yolunda: `make hepsi`, `duman` ve `cargo run` Sparkle'sız.
# Pakete `ditto --arch arm64` ile giriyor (tarball universal, paket yalnız
# Apple silicon) ve XPC servisleri atılıyor — Sparkle'ın belgesi onları
# yalnız sandbox'lı uygulamaya istiyor, bateri sandbox'lı değil (kabuk
# doğuruyor). Kalan iki yardımcı (`Autoupdate`, `Updater.app`) ve framework
# **içten dışa** bizim kimliğimizle yeniden imzalanıyor: dış paketin imzası
# içtekileri imzalamaz ve Sparkle ad-hoc imzalı geliyor. `--deep` yok
# (Sparkle'ın belgesi).
SPARKLE_VERSION = 2.10.0
SPARKLE_SHA256 = c2bf58aa8387266ac179357b1415d6f2635f044da8be41042af32425dae6da0c
SPARKLE_DIR = $(TARGET_DIR)/sparkle-$(SPARKLE_VERSION)
# Sparkle'ın okuduğu besleme; `Info.plist`'e `kur` anında yazılıyor.
# GitHub'ın `releases/latest/download/` adresi her zaman en yeni release'in
# aynı adlı dosyasını veriyor, yani sürüm yayınlamak güncellemeyi
# yayınlamak — sitenin sürümle hiçbir işi yok, site bir gün kapansa da kurulu
# kopyalar güncellenir. Denemede ezilir: `make paket FEED_URL=https://…`.
REPO = bateri/bateri
FEED_URL ?= https://github.com/$(REPO)/releases/latest/download/appcast.xml

sparkle:
	@test -f $(SPARKLE_DIR)/.verified && exit 0; \
	rm -rf $(SPARKLE_DIR) && mkdir -p $(SPARKLE_DIR) && \
	curl -fsSL -o $(SPARKLE_DIR).tar.xz \
		https://github.com/sparkle-project/Sparkle/releases/download/$(SPARKLE_VERSION)/Sparkle-$(SPARKLE_VERSION).tar.xz && \
	echo '$(SPARKLE_SHA256)  $(SPARKLE_DIR).tar.xz' | shasum -a 256 -c - >/dev/null || \
		{ echo "sparkle: Sparkle-$(SPARKLE_VERSION).tar.xz indirilemedi ya da özeti tutmuyor"; rm -f $(SPARKLE_DIR).tar.xz; exit 1; }; \
	tar -xJf $(SPARKLE_DIR).tar.xz -C $(SPARKLE_DIR) && rm -f $(SPARKLE_DIR).tar.xz && touch $(SPARKLE_DIR)/.verified

# Sürüm manifestten (`cargo metadata --no-deps`), `cargo pkgid`'den değil:
# pkgid Cargo.lock'u okuyor ve lock ancak derlemede güncelleniyor — sürüm
# yükseltildikten sonraki ilk `make yayin` eski numarayla paket çıkarıyordu
# (ölçüldü, güncelleme denemesinde).
VERSION = $(eval VERSION := $$(shell $(CARGO) metadata --format-version 1 --no-deps | sed -n 's/.*"name":"bateri","version":"\([^"]*\)".*/\1/p'))$(VERSION)
# İkonun adı tek yerde: şablonun `CFBundleIconFile`'ı.
ICON = $(eval ICON := $$(shell plutil -extract CFBundleIconFile raw assets/bundle/Info.plist.in))$(ICON)

kur: sparkle
	@case '$(APP)' in *[[:space:]]*) echo "kur: hedef dizininde boşluk var ($(APP)); tarif yolları bölerdi"; exit 1;; esac
	@# CFBundleVersion en çok üç noktalı tamsayı ister; `0.2.0-alpha.1` plutil'den geçer ama LaunchServices'ten geçmez.
	@echo '$(VERSION)' | grep -Eq '^[0-9]+(\.[0-9]+){0,2}$$' || { echo "kur: sürüm '$(VERSION)' CFBundleVersion biçiminde değil"; exit 1; }
	$(CARGO) build --release -p bateri
	rm -rf $(STAGE) $(ICONSET)
	mkdir -p $(STAGE)/Contents/MacOS $(STAGE)/Contents/Resources $(ICONSET)
	cp $(TARGET_DIR)/release/bateri $(STAGE)/Contents/MacOS/
	minos=$$(vtool -show-build $(STAGE)/Contents/MacOS/bateri | awk '$$1=="minos"{print $$2; exit}'); \
	sed -e 's/@VERSION@/$(VERSION)/g' -e "s/@MACOS_MIN@/$$minos/g" -e 's|@FEED_URL@|$(FEED_URL)|g' \
		assets/bundle/Info.plist.in > $(STAGE)/Contents/Info.plist
	@# iconutil standart on boyutu ister; `sips -s format icns` 1024'ü reddediyor.
	@for s in 16 32 128 256 512; do \
		sips -z $$s $$s assets/bundle/$(ICON).png --out $(ICONSET)/icon_$${s}x$${s}.png >/dev/null && \
		sips -z $$((s*2)) $$((s*2)) assets/bundle/$(ICON).png --out $(ICONSET)/icon_$${s}x$${s}@2x.png >/dev/null || exit 1; \
	done
	iconutil -c icns $(ICONSET) -o $(STAGE)/Contents/Resources/$(ICON).icns
	rm -rf $(ICONSET)
	@# GPL-3.0 §4: binary lisansın bir kopyasıyla gider; kaynağı kökteki LICENSE.
	cp LICENSE assets/bundle/Credits.html assets/bundle/THIRD-PARTY-LICENSES.txt $(STAGE)/Contents/Resources/
	@# Sarmalayıcı dosya dosya kopyalanıyor, `cp -R assets/shell` ile DEĞİL:
	@# ZDOTDIR bizim dizinimizi gösterdiği sürece oraya yazan bir kol (009
	@# phase-3'te `/etc/zshrc` bir kez `.zsh_history` doğurdu) ya da bir
	@# `.DS_Store` özyinelemeli kopyayla sessizce ürüne girerdi. Dizinin
	@# envanterinin TAM olarak bu beş dosya olduğunu `bundle_assets` sınar.
	mkdir -p $(STAGE)/Contents/Resources/shell/zsh
	cp assets/shell/zsh/.zshenv assets/shell/zsh/.zprofile assets/shell/zsh/.zshrc \
		assets/shell/zsh/.zlogin assets/shell/zsh/bateri.zsh \
		$(STAGE)/Contents/Resources/shell/zsh/
	mkdir -p $(STAGE)/Contents/Frameworks
	ditto --arch arm64 $(SPARKLE_DIR)/Sparkle.framework $(STAGE)/Contents/Frameworks/Sparkle.framework
	rm -rf $(STAGE)/Contents/Frameworks/Sparkle.framework/XPCServices \
		$(STAGE)/Contents/Frameworks/Sparkle.framework/Versions/B/XPCServices
	@# İmza en son ve içten dışa: paketin içine sonradan giren her bayt
	@# mührü bozardı, dıştaki imza da içtekileri mühürlüyor.
	@test '$(SIGN_ID)' = - || security find-identity -p codesigning | grep -qF '"$(SIGN_ID)"' || \
		{ echo "kur: anahtarlıkta '$(SIGN_ID)' adlı kod imzalama kimliği yok (security find-identity -p codesigning)"; exit 1; }
	$(SIGN) $(STAGE)/Contents/Frameworks/Sparkle.framework/Versions/B/Autoupdate
	$(SIGN) $(STAGE)/Contents/Frameworks/Sparkle.framework/Versions/B/Updater.app
	$(SIGN) $(STAGE)/Contents/Frameworks/Sparkle.framework
	$(SIGN) $(STAGE)
	codesign --verify --deep --strict $(STAGE)
	@c=$(STAGE)/Contents; fail() { echo "kur: içerik denetimi düştü — $$1"; exit 1; }; \
	key() { plutil -extract "$$1" raw $$c/Info.plist 2>/dev/null; }; \
	plutil -lint -s $$c/Info.plist || fail "Info.plist geçersiz"; \
	! grep -q '@[A-Z_]*@' $$c/Info.plist || fail "Info.plist'te doldurulmamış yer tutucu var"; \
	exe=$$c/MacOS/$$(key CFBundleExecutable); test -f $$exe && test -x $$exe || fail "CFBundleExecutable pakette yok"; \
	minos=$$(vtool -show-build $$exe | awk '$$1=="minos"{print $$2; exit}'); \
	test -n "$$minos" && test "$$(key LSMinimumSystemVersion)" = "$$minos" || fail "LSMinimumSystemVersion binary'nin minos'u ('$$minos') değil"; \
	test -s "$$c/Resources/$$(key CFBundleIconFile).icns" || fail "ikon pakette yok"; \
	test "$$(key CFBundleURLTypes.0.CFBundleURLSchemes.0)" = bateri || fail "URL şeması (bateri) Info.plist'te yok"; \
	test "$$(key SUFeedURL)" = '$(FEED_URL)' || fail "SUFeedURL '$(FEED_URL)' değil"; \
	test -n "$$(key SUPublicEDKey)" || fail "SUPublicEDKey Info.plist'te yok"; \
	fw=$$c/Frameworks/Sparkle.framework; \
	test -f $$fw/Versions/B/Sparkle && test -x $$fw/Versions/B/Autoupdate || fail "Sparkle.framework pakette yok"; \
	test ! -e $$fw/Versions/B/XPCServices || fail "Sparkle'ın XPC servisleri pakette kalmış"; \
	test "$$(lipo -archs $$fw/Versions/B/Sparkle)" = arm64 || fail "Sparkle arm64'e inceltilmemiş"; \
	team() { codesign -dv "$$1" 2>&1 | sed -n 's/^TeamIdentifier=//p'; }; \
	for x in $$fw $$fw/Versions/B/Autoupdate $$fw/Versions/B/Updater.app; do \
		test "$$(team $$x)" = "$$(team $(STAGE))" || fail "$$x paketle aynı kimlikle imzalı değil"; \
	done; \
	test '$(SIGN_ID)' = - || codesign -dv $(STAGE) 2>&1 | grep -q 'flags=.*runtime' || fail "hardened runtime yok"; \
	cmp -s LICENSE $$c/Resources/LICENSE || fail "LICENSE pakette yok ya da girdiden farklı"; \
	for f in Credits.html THIRD-PARTY-LICENSES.txt; do \
		cmp -s assets/bundle/$$f $$c/Resources/$$f || fail "$$f pakette yok ya da girdiden farklı"; \
	done; \
	for f in zsh/.zshenv zsh/.zprofile zsh/.zshrc zsh/.zlogin zsh/bateri.zsh; do \
		cmp -s assets/shell/$$f $$c/Resources/shell/$$f || fail "shell/$$f pakette yok ya da girdiden farklı"; \
	done; \
	rm -rf $(APP) && mv $(STAGE) $(APP) && \
	echo "kur: $(APP) (sürüm $(VERSION), taban macOS $$minos, imza $(if $(filter -,$(SIGN_ID)),ad-hoc,'$(SIGN_ID)'))"

# Girdisi henüz olmayan hedefler. Var olurlar ki `proje.md`'nin doğrulama
# tablosu var olmayan bir hedef adı taşımasın; koşarlarsa "henüz yok" deyip
# kırmızı düşerler, "geçti" demezler. make reçete hatasını 2 ile döndürür;
# ayırt edici sinyal stdout'taki "henüz yok" metnidir. Hedef gerçek olunca
# satırı sil, proje.md başındaki listeden de çıkar.
henuz_yok = @echo "henüz yok: $(1)"; exit 1


# Başka bir Mac'e gönderilecek zip: `kur`'un denetlenmiş paketini
# `ditto` ile sıkıştırır (Finder'ın "Sıkıştır"ıyla aynı biçim; `zip -r`
# macOS'un genişletilmiş özniteliklerini ve imza mührünü bozabiliyor).
# Adında sürüm var, eski bir zip yanlışlıkla gönderilmesin.
#
# **Developer ID'yle notarize ediliyor**: paket zip'lenip Apple'a gidiyor
# (`notarytool submit --wait`, anahtarlıktaki `$(NOTARY_PROFILE)` profili —
# kurulumu `xcrun notarytool store-credentials`, parola depoya girmiyor),
# kabul edilirse bilet pakete **zımbalanıyor** (`stapler`) ve zip ondan
# SONRA yeniden alınıyor: bilet paketin içinde, zımbasız zip'i çevrimdışı
# açan Mac Apple'a soramaz ve uyarıya düşerdi. Kapı `spctl`'ın
# "Notarized Developer ID" demesi. Başka kimlikte notarization yok ve alıcı
# ilk açılışta Gatekeeper uyarısını Sistem Ayarları → Gizlilik ve Güvenlik →
# "Yine de Aç" ile geçer.
#
# **İki çıktı, iki okur.** Zip Sparkle'ın: güncellemede kullanıcı hiçbir şey
# sürüklemiyor ve zip küçük; beslemenin öğesi onu sürümüyle adlandırıyor.
# DMG ilk kurulumun ve adında **sürüm yok**: GitHub'ın
# `releases/latest/download/bateri.dmg` bağlantısı böylece hiç bayatlamıyor
# ve site onu olduğu gibi gösteriyor. İlk kurulumda Finder penceresi
# uygulamayı Applications'a sürükleten tek bir iş gösteriyor (arka plan,
# ok, Applications kısayolu; düzen `assets/dmg/settings.py`, görsel
# `tools/dmg_background.py`). Zip'le gelen uygulama İndirilenler'de kalıp
# oradan açılıyordu; macOS öyle açılan uygulamayı salt okunur bir kopyadan
# koşturuyor (App Translocation) ve Sparkle onu güncelleyemiyor. DMG,
# içindeki zımbalı uygulamadan kuruluyor ve kendisi de ayrıca imzalanıp
# notarize ediliyor, zımbalanıyor — indirilen DMG'yi açan Mac de soru
# sormamalı.
#
# DMG'yi `dmgbuild` kuruyor (sürümü ve bağımlılıkları sabit, ilk koşuda
# `$(DMGBUILD_DIR)`'e bir venv'e iniyor, depoya girmiyor): Finder'ın düzen
# dosyasını (`.DS_Store`) kendisi yazıyor, yani AppleScript'le Finder'ı
# sürmüyor — başsız ve izin sormadan koşuyor.
ZIP = $(TARGET_DIR)/release/bateri-$(VERSION).zip
DMG = $(TARGET_DIR)/release/bateri.dmg
NOTARY_PROFILE ?= bateri-notary
DMGBUILD_VERSION = 1.6.7
DMGBUILD_DIR = $(TARGET_DIR)/dmgbuild-$(DMGBUILD_VERSION)

dmgbuild:
	@test -x $(DMGBUILD_DIR)/bin/dmgbuild && exit 0; \
	rm -rf $(DMGBUILD_DIR) && python3 -m venv $(DMGBUILD_DIR) && \
	$(DMGBUILD_DIR)/bin/pip install -q --disable-pip-version-check \
		dmgbuild==$(DMGBUILD_VERSION) ds_store==1.3.3 mac_alias==2.2.3 || \
		{ echo "dmgbuild: kurulamadı (python3 -m venv + pip, ağ ister)"; rm -rf $(DMGBUILD_DIR); exit 1; }

# Notarize edip zımbalar: $(1) Apple'a giden dosya, $(2) zımbalanacak öğe.
# Developer ID dışındaki kimlikte hiç çağrılmıyor.
define notarize
	out=$$(xcrun notarytool submit $(1) --keychain-profile '$(NOTARY_PROFILE)' --wait 2>&1) || true; \
	echo "$$out" | tail -n 2; \
	echo "$$out" | grep -q 'status: Accepted' || { \
		id=$$(echo "$$out" | sed -n 's/^ *id: //p' | head -n 1); \
		echo "paket: notarization kabul edilmedi$${id:+ — ayrıntı: xcrun notarytool log $$id --keychain-profile $(NOTARY_PROFILE)}"; exit 1; }; \
	xcrun stapler staple -q $(2); \
	xcrun stapler validate -q $(2)
endef

paket: kur dmgbuild
	codesign --verify --deep --strict $(APP)
	rm -f $(ZIP) $(ZIP).notary $(DMG)
	@if printf '%s' '$(SIGN_ID)' | grep -q '^Developer ID Application:'; then \
		set -e; \
		ditto -c -k --sequesterRsrc --keepParent $(APP) $(ZIP).notary; \
		echo "paket: uygulama notarization için Apple'a gönderiliyor (birkaç dakika sürebilir)"; \
		$(call notarize,$(ZIP).notary,$(APP)); \
		rm -f $(ZIP).notary; \
		spctl -a -vv -t exec $(APP) 2>&1 | grep -q 'source=Notarized Developer ID' || \
			{ echo "paket: spctl paketi notarize görmüyor"; spctl -a -vv -t exec $(APP); exit 1; }; \
	else \
		echo "paket: notarize EDİLMEDİ — imza '$(SIGN_ID)', Developer ID değil"; \
	fi
	ditto -c -k --sequesterRsrc --keepParent $(APP) $(ZIP)
	$(DMGBUILD_DIR)/bin/dmgbuild -s assets/dmg/settings.py -D app=$(APP) \
		-D icon=$(APP)/Contents/Resources/$(ICON).icns -D here=$(CURDIR)/assets/dmg bateri $(DMG)
	@if printf '%s' '$(SIGN_ID)' | grep -q '^Developer ID Application:'; then \
		set -e; \
		codesign --force --sign '$(SIGN_ID)' --timestamp $(DMG); \
		echo "paket: DMG notarization için Apple'a gönderiliyor"; \
		$(call notarize,$(DMG),$(DMG)); \
		spctl -a -vv -t open --context context:primary-signature $(DMG) 2>&1 | grep -q 'source=Notarized Developer ID' || \
			{ echo "paket: spctl DMG'yi notarize görmüyor"; spctl -a -vv -t open --context context:primary-signature $(DMG); exit 1; }; \
	fi
	@echo "paket: $(ZIP) + $(DMG) ($$(lipo -archs $(APP)/Contents/MacOS/bateri), macOS $$(plutil -extract LSMinimumSystemVersion raw $(APP)/Contents/Info.plist)+)"

# Bir sürümü yayınlamak tek komut, bu Mac'te:
#
#   make gonder      yayin + main'i push + yayinla — baştan sona
#
# İki yarısı ayrıca da çağrılabiliyor, çünkü bölünmenin kendisi amaç: zip
# herkese açılmadan önce bu Mac'te denenir.
#
#   make yayin       kapılar, paket (derle, imzala, notarize et, zımbala),
#                    sürüm notu ve besleme → $(RELEASE_DIR); Apple'ın
#                    notarization'ı dışında makineden hiçbir şey çıkmıyor
#   make yayinla     derlenen commit'i v<sürüm> diye etiketler, etiketi
#                    push eder ve GitHub release'ini zip, DMG ve beslemeyle
#                    açar (`gh`)
#
# **Önce kapılar, sonra paket** (`yayin-kapisi`; paket dakikalar sürüyor ve
# kapı ucuz): sürüm bilinen bir koddan çıkar — ağaç temiz ve `v$(VERSION)`
# ne burada ne origin'de var — ve sürüm notuyla çıkar. Dal sorulmuyor: asıl
# güvence `yayinla`'nın "commit origin/main'de" kapısı, yani main'e giden bir
# commit başka bir worktree'de de paketlenebilir. Notun
# tek kaynağı kökteki `CHANGELOG.md` (Keep a Changelog, İngilizce): kapı
# `## [$(VERSION)]` bölümünü kesiyor, bölüm yoksa ya da boşsa paket
# derlenmeden duruyor. Not hem release sayfasında hem Sparkle'ın "yeni sürüm
# var" penceresinde görünüyor — notsuz pencere kullanıcıyı neyi kurduğunu
# bilmeden "Install"a itiyordu. Sürüm numarası `Cargo.toml`'un; kapı onu
# okuyor, `VERSION=` vermek gerekmiyor.
#
# `yayin` derlediği commit'i `$(RELEASE_DIR)/commit`'e yazıyor ve `yayinla`
# **o** commit'i etiketliyor, o an HEAD neyse onu değil — etiket her zaman
# zip'in içindeki kodu adlandırıyor. Etiket tek başına push edilmiyor:
# commit origin/main'de değilse `yayinla` duruyor, yoksa hiçbir dalın
# tutmadığı bir commit yayınlanırdı.
#
# **Besleme tek öğeli.** `releases/latest/download/appcast.xml` her zaman en
# yeni release'inkini veriyor, yani okunan tek öğe en yenisi; eski
# sürümlerin öğesi hiçbir kurulu kopyaya ulaşmazdı. `generate_appcast`
# yalnız bu sürümün zip'ini ve notunu tutan geçici bir klasörden koşuyor,
# zip'i gizli EdDSA anahtarıyla imzalıyor (anahtar anahtarlıkta,
# `generate_keys`; kaybolursa kurulu kopyalara bir daha güncelleme
# gönderilemez — yedeği `generate_keys -x`) ve indirme adresi release'in
# kendi adresi (`releases/download/v<sürüm>/`). Delta güncelleme yok
# (`--maximum-deltas 0`): paket küçük. Notarize edilmemiş paket yayına
# girmiyor: Sparkle'ın indirdiği kopya da Gatekeeper'dan geçmek zorunda.
#
# Hatalı bir sürümün geri alınışı **yeni bir sürüm**: Sparkle eski sürüme
# inmiyor ve bir release'i silmek `latest`'i bir öncekine döndürür ama o
# sürümü kurmuş kopyaları olduğu yerde bırakır.
RELEASE_DIR = $(TARGET_DIR)/release/v$(VERSION)
NOTES = $(RELEASE_DIR)/notes.md
RELEASE_URL = https://github.com/$(REPO)/releases/download/v$(VERSION)/

yayin-kapisi:
	@echo '$(VERSION)' | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$$' || { echo "yayin: sürüm '$(VERSION)' x.y.z değil (Cargo.toml)"; exit 1; }
	@test -z "$$(git status --porcelain)" || { echo "yayin: çalışma ağacı temiz değil, sürüm bilinen bir koddan çıkmalı"; exit 1; }
	@! git rev-parse -q --verify 'refs/tags/v$(VERSION)' >/dev/null || { echo "yayin: v$(VERSION) etiketi zaten var — Cargo.toml'da sürümü yükselt"; exit 1; }
	@! git ls-remote --exit-code --tags origin 'refs/tags/v$(VERSION)' >/dev/null || { echo "yayin: v$(VERSION) etiketi origin'de zaten var"; exit 1; }
	@rm -rf $(RELEASE_DIR) && mkdir -p $(RELEASE_DIR); \
	awk -v v='$(VERSION)' '/^## \[/ { if (on) exit; if (index($$0, "## [" v "]") == 1) { on = 1; next } } on' CHANGELOG.md \
		| awk 'NF { p = 1 } p' | awk '{ l[NR] = $$0 } END { n = NR; while (n > 0 && l[n] ~ /^[[:space:]]*$$/) n--; for (i = 1; i <= n; i++) print l[i] }' > $(NOTES); \
	grep -q '[^[:space:]]' $(NOTES) || { echo "yayin: CHANGELOG.md'de '## [$(VERSION)]' bölümü yok ya da boş (Unreleased'i sürüme çevir)"; exit 1; }

yayin: yayin-kapisi paket
	@spctl -a -vv -t exec $(APP) 2>&1 | grep -q 'source=Notarized Developer ID' && xcrun stapler validate -q $(APP) || \
		{ echo "yayin: paket notarize değil, yayına girmez"; exit 1; }
	@spctl -a -vv -t open --context context:primary-signature $(DMG) 2>&1 | grep -q 'source=Notarized Developer ID' || \
		{ echo "yayin: DMG notarize değil, yayına girmez"; exit 1; }
	@test "$$(plutil -extract SUFeedURL raw $(APP)/Contents/Info.plist)" = '$(FEED_URL)' || { echo "yayin: paketin beslemesi $(FEED_URL) değil"; exit 1; }
	cp $(ZIP) $(DMG) $(RELEASE_DIR)/
	rm -rf $(RELEASE_DIR)/feed && mkdir -p $(RELEASE_DIR)/feed
	cp $(ZIP) $(RELEASE_DIR)/feed/
	cp $(NOTES) $(RELEASE_DIR)/feed/bateri-$(VERSION).md
	$(SPARKLE_DIR)/bin/generate_appcast --maximum-deltas 0 --embed-release-notes \
		--download-url-prefix '$(RELEASE_URL)' -o $(RELEASE_DIR)/appcast.xml $(RELEASE_DIR)/feed
	rm -rf $(RELEASE_DIR)/feed
	@test "$$(grep -c '<item>' $(RELEASE_DIR)/appcast.xml)" = 1 && grep -q 'url="$(RELEASE_URL)bateri-$(VERSION).zip"' $(RELEASE_DIR)/appcast.xml \
		&& grep -q 'sparkle:edSignature=' $(RELEASE_DIR)/appcast.xml || { echo "yayin: besleme tek, imzalı ve release'i gösteren bir öğe değil"; exit 1; }
	git rev-parse HEAD > $(RELEASE_DIR)/commit
	@echo "yayin: $(RELEASE_DIR) ($$(cat $(RELEASE_DIR)/commit)) — $(APP)'i dene, sonra: make yayinla"

yayinla:
	@for f in bateri-$(VERSION).zip bateri.dmg appcast.xml notes.md commit; do \
		test -f $(RELEASE_DIR)/$$f || { echo "yayinla: $(RELEASE_DIR)/$$f yok — önce make yayin"; exit 1; }; \
	done
	@! git ls-remote --exit-code --tags origin 'refs/tags/v$(VERSION)' >/dev/null || { echo "yayinla: v$(VERSION) etiketi origin'de zaten var"; exit 1; }
	git fetch -q origin main
	@git merge-base --is-ancestor "$$(cat $(RELEASE_DIR)/commit)" origin/main || { echo "yayinla: derlenen commit origin/main'de değil — önce main'i push et"; exit 1; }
	git tag -a 'v$(VERSION)' -m 'bateri $(VERSION)' "$$(cat $(RELEASE_DIR)/commit)"
	git push origin 'v$(VERSION)'
	gh release create 'v$(VERSION)' $(RELEASE_DIR)/bateri.dmg $(RELEASE_DIR)/bateri-$(VERSION).zip $(RELEASE_DIR)/appcast.xml \
		--repo '$(REPO)' --verify-tag --latest --title 'bateri $(VERSION)' --notes-file $(NOTES)
	@echo "yayinla: v$(VERSION) — https://github.com/$(REPO)/releases/tag/v$(VERSION)"

# main dışında notarization beklemesinden ÖNCE duruyor, sonra değil:
# `yayinla` commit'i origin/main'de istiyor ve burada yalnız main push ediliyor.
gonder:
	@test "$$(git branch --show-current)" = main || { echo "gonder: dal main değil"; exit 1; }
	$(MAKE) yayin
	git push origin main
	$(MAKE) yayinla

# Bu Mac'e kurar: `kur`'un denetlenmiş paketini `$(INSTALL_DIR)`'a koyar.
# Eski paketin üstüne `ditto` ile yazılmıyor, çünkü `ditto` birleştirir ve
# yeni sürümde silinmiş bir dosya eski paketten kalırdı. Kopya önce yanda
# geçici bir ada iniyor ve eskinin yerine ancak kopya bittiğinde geçiyor:
# yarıda kesilen bir kurulum çalışan paketi yok etmesin. Açık bir bateri
# varken durur ve kapatmaz — kullanıcının oturumundaki kabukları öldürmek
# bir derleme hedefinin kararı değil.
INSTALL_DIR ?= /Applications
INSTALLED = $(INSTALL_DIR)/bateri.app

yukle: kur
	@if pgrep -f '$(INSTALLED)/Contents/MacOS/bateri' >/dev/null; then \
		echo "yukle: $(INSTALLED) açık — önce kapat (⌘Q), sonra yeniden dene"; exit 1; fi
	rm -rf $(INSTALLED).new
	ditto $(APP) $(INSTALLED).new
	codesign --verify --deep --strict $(INSTALLED).new
	rm -rf $(INSTALLED)
	mv $(INSTALLED).new $(INSTALLED)
	@echo "yukle: $(INSTALLED)"

terminfo:
	$(call henuz_yok,assets/terminfo bir shell/TERM setiyle gelir)

# bt-core'un Linux kapısı: `clippy -D warnings` ve `test`, Docker'da,
# `tools/linux/Dockerfile`'ın imajında, `--locked` (Cargo.lock'u değiştiren
# koşu kırmızı düşer, sessizce yeni sürüm çözmez). CLAUDE.md'nin "bt-core
# platformsuz, kapı Linux hedefiyle derlemedir" sözü bu komuttur.
# `make hepsi`'nin DIŞINDA, çünkü Docker ister ve ilk koşusu imajı kurup bütün
# grafı Linux için derler; ne zaman koştuğu `.claude/is-akisi/proje.md` →
# Doğrulama'da (Linux'ta derlenen bir crate değiştiyse).
# Kapsam setlerle büyür — bugün yalnız bt-core; sırası (bt-atlas + lavapipe
# üstünde bt-gpu, sonra bt-shell-common) `docs/YOL-HARITASI.md`'de. Büyürken
# `-p` listesi ve imaj tarifi birlikte değişir.
# Sıra:
# 1. Sürüm: yerel `rustc`'nin major.minor'ü imaj etiketininkiyle aynı değilse
#    KIRMIZI (exit 1) — iki derleyicinin clippy'si iki ayrı kapıdır. Bu bir
#    "koşamadı" değildir: çaresi Dockerfile'ın `FROM` satırını güncellemek.
# 2. Docker yoksa ya da daemon cevap vermiyorsa stdout'a "ATLANDI" basıp 78
#    ile çıkar; make bunu 2 olarak döndürür — `make duman`'daki gibi ayırt
#    edici sinyal stdout metnidir, çıkış kodu değil. Doğrulamada `[~]` yalnız
#    bu kolda yazılır.
# 3. İmajı kurar (katman önbellekli) ve konteynerde koşar: depo `/w`'ye bağlı,
#    çıktılar `target/linux`'a (macOS derlemesiyle karışmasın; `/target/`
#    zaten .gitignore'da), crate indirmeleri adlı bir volume'da.
LINUX_DOCKERFILE = tools/linux/Dockerfile
LINUX_RUST = $(shell sed -n 's/^FROM rust:\([0-9]*\.[0-9]*\)-.*/\1/p' $(LINUX_DOCKERFILE))
LINUX_IMAGE = bateri-linux:$(LINUX_RUST)
LINUX_CRATES = -p bt-core

linux:
	@yerel=$$(rustc --version | sed -n 's/^rustc \([0-9]*\.[0-9]*\).*/\1/p'); \
	if [ -z "$(LINUX_RUST)" ]; then \
		echo "linux: $(LINUX_DOCKERFILE)'ın FROM satırından rust sürümü okunamadı"; exit 1; fi; \
	if [ "$$yerel" != "$(LINUX_RUST)" ]; then \
		echo "linux: yerel rustc $$yerel, imaj rust:$(LINUX_RUST) — sürümler uyuşmuyor; $(LINUX_DOCKERFILE)'ın FROM satırını yerel sürüme çek"; exit 1; fi
	@if ! command -v docker >/dev/null 2>&1 || ! docker info >/dev/null 2>&1; then \
		echo "ATLANDI: Docker yok ya da daemon cevap vermiyor — make linux koşamadı"; exit 78; fi
	docker build -q -t $(LINUX_IMAGE) -f $(LINUX_DOCKERFILE) tools/linux
	docker run --rm -v "$(CURDIR)":/w -v bateri-linux-cargo:/usr/local/cargo/registry \
		-e CARGO_TARGET_DIR=/w/target/linux $(LINUX_IMAGE) sh -c '\
		cargo clippy $(LINUX_CRATES) --all-targets --locked -- -D warnings && \
		cargo test $(LINUX_CRATES) --locked'
