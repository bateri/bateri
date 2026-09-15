CARGO ?= cargo

# Prerequisite sırası yalnız seri make'te garantidir; -j altında "en ucuz kapı
# önce" ve "sürüm başta" sözü bozulur.
.NOTPARALLEL:
.PHONY: hepsi fmt denetim clippy test shader duman terminfo test-yaris kur

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
# - Bağımlılık DÜŞÜRMEZ, uyarır: bilinçli bir bağımlılık kararı da Cargo.lock'u
#   değiştirir; kararın kaydını `/audit` arar.
denetim:
	@fail=0; \
	if $(CARGO) tree -p bt-core -e normal | grep -E "objc2|core-text|core-graphics|metal"; then echo "denetim: bt-core platform kütüphanesine bağlanıyor"; fail=1; fi; \
	if $(CARGO) tree -p bt-atlas -e normal | grep -E "(^|[ ─])objc2 v"; then echo "denetim: bt-atlas objc2 çekirdeğine bağlanıyor"; fail=1; fi; \
	if $(CARGO) tree -p bt-gpu -e normal | grep -E "bt-shell"; then echo "denetim: bt-gpu yukarı, bt-shell'e bağlanıyor"; fail=1; fi; \
	if grep -rn "objc2\|core_text\|core_graphics" crates/bt-core/src | grep -v ":[[:space:]]*//"; then echo "denetim: bt-core kaynağında platform çağrısı var"; fail=1; fi; \
	for f in crates/bt-core/src/*.rs; do \
		awk -v file="$$f" '/^[[:space:]]*#\[cfg\(test\)\]/{exit} /\.unwrap\(\)|\.expect\(|panic!|unreachable!/ && !/\/\/ audit: / && !/^[[:space:]]*\/\//{print file":"NR": "$$0; hit=1} END{exit hit}' "$$f" \
			|| { echo "denetim: bt-core'da gerekçesiz panik yolu ($$f)"; fail=1; }; \
	done; \
	if [ -d assets/shell ] && grep -rnE "(>>?|sed -i|tee).*(\.zshrc|\.zprofile|\.bashrc|\.bash_profile|\.profile|config\.fish)" assets/shell; then echo "denetim: shell entegrasyonu kullanıcı rc dosyasına yazıyor"; fail=1; fi; \
	git diff --quiet HEAD -- Cargo.lock $$(git ls-files '*Cargo.toml') || echo "denetim: uyarı — Cargo.toml/Cargo.lock HEAD'den farklı; bağımlılık kararı kayıtlı mı?"; \
	test $$fail -eq 0 && echo "denetim: temiz"

clippy:
	$(CARGO) clippy --workspace --all-targets -- -D warnings

test:
	$(CARGO) test --workspace

# Pencereyi açar, BT_RUN_SECONDS dolunca kare, hücre, glyph, kural çizgisi ve
# atlas yuvası sayısına bakar:
# kare=N hucre=K glif=G kural=R yuva=U/T yuk=smoke istek=I kapanis=clean \
#   profil=debug ornek=off pipeline=ok
# İlk dördünden (kare, hucre, glif, kural) BİRİ 0 ise kırmızı; `yuva`, `yuk`,
# `istek` ve `profil` kapı değil, sayaç ve etiket.
# `kare` ayrıca ÜST SINIRLI (bkz. app.rs IDLE_FRAME_LIMIT): boşta sıfır kareyi
# bozan değişikliğin belirtisi eksik kare değil FAZLA karedir. `yuva` bir kapı
# değil sayaç — eşiği ölçülmedi, ölçülmemiş sayı kapıya yazılmaz.
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
duman:
	env -u BT_SCROLL_TEST -u BT_FRAME_STATS BT_RUN_SECONDS=3 $(CARGO) run -q -p bateri

# build.rs'in yaptığını cargo'nun bayatlık takibini atlayarak koşturur;
# derleme reçetesi burada TEKRARLANMAZ.
shader:
	touch $(wildcard crates/bt-gpu/shaders/*.metal)
	$(CARGO) build -p bt-gpu

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

# Release derler ve `bateri.app`'i target/ altında kurar (/Applications'a
# DEĞİL). İmza, notarization ve Sparkle yok (006 Karar 6): yerel kopya
# linker'ın ad-hoc imzasıyla açılıyor, `codesign -vv` paketi "no resources"
# diye reddediyor ve bu beklenen hâl.
#
# Paket önce `$(STAGE)`'de kurulur ve yalnız denetimden geçerse `$(APP)`'in
# yerine taşınır: yerinde kurulsaydı düşen bir koşu Dock'un gösterdiği yolda
# lisanssız ya da ikonsuz, açılabilir bir paket bırakırdı.
#
# Girdilerin İÇERİĞİNİ (`assets/bundle/`) `make hepsi` içindeki
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
# `cargo pkgid` sürümü `…#0.1.0` ya da `…#bateri@0.1.0` biçiminde verir;
# sed son ayırıcıya kadar siler. Ayırıcıyı adıyla yazamıyoruz: make o
# karakteri satırın içinde de yorum başı sayıyor.
VERSION = $(eval VERSION := $$(shell $(CARGO) pkgid -p bateri | sed 's/.*[^0-9A-Za-z.+-]//'))$(VERSION)
# İkonun adı tek yerde: şablonun `CFBundleIconFile`'ı.
ICON = $(eval ICON := $$(shell plutil -extract CFBundleIconFile raw assets/bundle/Info.plist.in))$(ICON)

kur:
	@case '$(APP)' in *[[:space:]]*) echo "kur: hedef dizininde boşluk var ($(APP)); tarif yolları bölerdi"; exit 1;; esac
	@# CFBundleVersion en çok üç noktalı tamsayı ister; `0.2.0-alpha.1` plutil'den geçer ama LaunchServices'ten geçmez.
	@echo '$(VERSION)' | grep -Eq '^[0-9]+(\.[0-9]+){0,2}$$' || { echo "kur: sürüm '$(VERSION)' CFBundleVersion biçiminde değil"; exit 1; }
	$(CARGO) build --release -p bateri
	rm -rf $(STAGE) $(ICONSET)
	mkdir -p $(STAGE)/Contents/MacOS $(STAGE)/Contents/Resources $(ICONSET)
	cp $(TARGET_DIR)/release/bateri $(STAGE)/Contents/MacOS/
	minos=$$(vtool -show-build $(STAGE)/Contents/MacOS/bateri | awk '$$1=="minos"{print $$2; exit}'); \
	sed -e 's/@VERSION@/$(VERSION)/g' -e "s/@MACOS_MIN@/$$minos/g" \
		assets/bundle/Info.plist.in > $(STAGE)/Contents/Info.plist
	@# iconutil standart on boyutu ister; `sips -s format icns` 1024'ü reddediyor.
	@for s in 16 32 128 256 512; do \
		sips -z $$s $$s assets/bundle/$(ICON).png --out $(ICONSET)/icon_$${s}x$${s}.png >/dev/null && \
		sips -z $$((s*2)) $$((s*2)) assets/bundle/$(ICON).png --out $(ICONSET)/icon_$${s}x$${s}@2x.png >/dev/null || exit 1; \
	done
	iconutil -c icns $(ICONSET) -o $(STAGE)/Contents/Resources/$(ICON).icns
	rm -rf $(ICONSET)
	cp assets/bundle/Credits.html assets/bundle/THIRD-PARTY-LICENSES.txt $(STAGE)/Contents/Resources/
	@c=$(STAGE)/Contents; fail() { echo "kur: içerik denetimi düştü — $$1"; exit 1; }; \
	key() { plutil -extract "$$1" raw $$c/Info.plist 2>/dev/null; }; \
	plutil -lint -s $$c/Info.plist || fail "Info.plist geçersiz"; \
	! grep -q '@[A-Z_]*@' $$c/Info.plist || fail "Info.plist'te doldurulmamış yer tutucu var"; \
	exe=$$c/MacOS/$$(key CFBundleExecutable); test -f $$exe && test -x $$exe || fail "CFBundleExecutable pakette yok"; \
	minos=$$(vtool -show-build $$exe | awk '$$1=="minos"{print $$2; exit}'); \
	test -n "$$minos" && test "$$(key LSMinimumSystemVersion)" = "$$minos" || fail "LSMinimumSystemVersion binary'nin minos'u ('$$minos') değil"; \
	test -s "$$c/Resources/$$(key CFBundleIconFile).icns" || fail "ikon pakette yok"; \
	for f in Credits.html THIRD-PARTY-LICENSES.txt; do \
		cmp -s assets/bundle/$$f $$c/Resources/$$f || fail "$$f pakette yok ya da girdiden farklı"; \
	done; \
	rm -rf $(APP) && mv $(STAGE) $(APP) && \
	echo "kur: $(APP) (sürüm $(VERSION), taban macOS $$minos)"

# Girdisi henüz olmayan hedefler. Var olurlar ki `proje.md`'nin doğrulama
# tablosu var olmayan bir hedef adı taşımasın; koşarlarsa "henüz yok" deyip
# kırmızı düşerler, "geçti" demezler. make reçete hatasını 2 ile döndürür;
# ayırt edici sinyal stdout'taki "henüz yok" metnidir. Hedef gerçek olunca
# satırı sil, proje.md başındaki listeden de çıkar.
henuz_yok = @echo "henüz yok: $(1)"; exit 1

terminfo:
	$(call henuz_yok,assets/terminfo bir shell/TERM setiyle gelir)
