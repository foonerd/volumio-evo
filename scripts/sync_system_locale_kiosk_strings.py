#!/usr/bin/env python3
"""
Merge translated SYSTEM.* kiosk/locale strings into every strings_<lang>.json (except en).

Run from repo root: python3 scripts/sync_system_locale_kiosk_strings.py

Source of truth for English keys: layer/web/classic/app/i18n/strings_en.json (SYSTEM subsection).
"""

from __future__ import annotations

import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
THEMES = [
    ROOT / "layer/web/classic/app/i18n",
    ROOT / "layer/web/contemporary/app/i18n",
    ROOT / "layer/web/manifest/app/i18n",
]

# Keys must match strings_en.json SYSTEM.*
TRANSLATIONS: dict[str, dict[str, str]] = {
    "ca": {
        "LOCALE_REGION": "Idioma, regió i zona horària",
        "COUNTRY_DOC": "Defineix el domini regulatori Wi‑Fi (iw reg) segons el lloc d’ús.",
        "KIOSK_WPE": "Quiosc i pantalla (WPE)",
        "KIOSK_WPE_DOC": "Reservat per a la shell Wayland/WPE al dispositiu; vegeu docs/KIOSK.md.",
        "KIOSK_ENABLE": "Activa la interfície de quiosc",
        "PRIMARY_DISPLAY": "Pantalla principal",
        "PRIMARY_DISPLAY_DOC": "Quina sortida mostra la interfície a pantalla completa amb el quiosc activat.",
    },
    "cs": {
        "LOCALE_REGION": "Jazyk, region a časové pásmo",
        "COUNTRY_DOC": "Nastaví regulační doménu Wi‑Fi (iw reg) podle země použití.",
        "KIOSK_WPE": "Kiosk a displej (WPE)",
        "KIOSK_WPE_DOC": "Vyhrazeno pro Wayland/WPE shell na zařízení; viz docs/KIOSK.md.",
        "KIOSK_ENABLE": "Zapnout kiosk rozhraní",
        "PRIMARY_DISPLAY": "Primární displej",
        "PRIMARY_DISPLAY_DOC": "Který výstup zobrazuje celoobrazové rozhraní při zapnutém kiosku.",
    },
    "da": {
        "LOCALE_REGION": "Sprog, region og tidszone",
        "COUNTRY_DOC": "Angiver Wi‑Fi‑regulatory domæne (iw reg) for brugsstedet.",
        "KIOSK_WPE": "Kiosk og skærm (WPE)",
        "KIOSK_WPE_DOC": "Reserveret til Wayland/WPE‑shell på enheden; se docs/KIOSK.md.",
        "KIOSK_ENABLE": "Aktivér kiosk‑brugerflade",
        "PRIMARY_DISPLAY": "Primær skærm",
        "PRIMARY_DISPLAY_DOC": "Hvilken udgang viser fuldskærms‑UI når kiosk er aktiv.",
    },
    "de": {
        "LOCALE_REGION": "Sprache, Region und Zeit",
        "COUNTRY_DOC": "Legt die WLAN-Regulatory-Domain fest (iw reg) für den Einsatzort des Geräts.",
        "KIOSK_WPE": "Kiosk & Anzeige (WPE)",
        "KIOSK_WPE_DOC": "Reserviert für die Wayland-/WPE-Shell auf dem Gerät; siehe docs/KIOSK.md.",
        "KIOSK_ENABLE": "Kiosk-Oberfläche aktivieren",
        "PRIMARY_DISPLAY": "Primäre Anzeige",
        "PRIMARY_DISPLAY_DOC": "Welcher Ausgang zeigt die Vollbild-Oberfläche bei aktivierter Kiosk-Funktion.",
    },
    "es": {
        "LOCALE_REGION": "Idioma, región y zona horaria",
        "COUNTRY_DOC": "Define el dominio regulatorio Wi‑Fi (iw reg) según el país de uso.",
        "KIOSK_WPE": "Quiosco y pantalla (WPE)",
        "KIOSK_WPE_DOC": "Reservado para el shell Wayland/WPE en el dispositivo; véase docs/KIOSK.md.",
        "KIOSK_ENABLE": "Activar interfaz de quiosco",
        "PRIMARY_DISPLAY": "Pantalla principal",
        "PRIMARY_DISPLAY_DOC": "Qué salida muestra la interfaz a pantalla completa con el quiosco activado.",
    },
    "fi": {
        "LOCALE_REGION": "Kieli, alue ja aikavyöhyke",
        "COUNTRY_DOC": "Asettaa Wi‑Fi‑sääntelyalueen (iw reg) käyttöpaikan mukaan.",
        "KIOSK_WPE": "Kioski ja näyttö (WPE)",
        "KIOSK_WPE_DOC": "Varattu laitteen Wayland/WPE‑kuorelle; katso docs/KIOSK.md.",
        "KIOSK_ENABLE": "Ota kioski‑käyttöliittymä käyttöön",
        "PRIMARY_DISPLAY": "Ensisijainen näyttö",
        "PRIMARY_DISPLAY_DOC": "Kumpi lähtö näyttää koko näytön käyttöliittymän, kun kioski on käytössä.",
    },
    "fr": {
        "LOCALE_REGION": "Langue, région et fuseau horaire",
        "COUNTRY_DOC": "Définit le domaine réglementaire Wi‑Fi (iw reg) selon le lieu d'utilisation.",
        "KIOSK_WPE": "Kiosque et affichage (WPE)",
        "KIOSK_WPE_DOC": "Réservé au shell Wayland/WPE sur l'appareil ; voir docs/KIOSK.md.",
        "KIOSK_ENABLE": "Activer l'interface kiosque",
        "PRIMARY_DISPLAY": "Écran principal",
        "PRIMARY_DISPLAY_DOC": "Sortie vidéo qui affiche l'interface plein écran lorsque le mode kiosque est activé.",
    },
    "gr": {
        "LOCALE_REGION": "Γλώσσα, περιοχή και ζώνη ώρας",
        "COUNTRY_DOC": "Ορίζει τον κανονιστικό τομέα Wi‑Fi (iw reg) για τη χώρα χρήσης.",
        "KIOSK_WPE": "Κιόσκι και οθόνη (WPE)",
        "KIOSK_WPE_DOC": "Δεσμευμένο για το κέλυφος Wayland/WPE στη συσκευή· δείτε docs/KIOSK.md.",
        "KIOSK_ENABLE": "Ενεργοποίηση διεπαφής κιόσκι",
        "PRIMARY_DISPLAY": "Κύρια οθόνη",
        "PRIMARY_DISPLAY_DOC": "Ποια έξοδος εμφανίζει πλήρη οθόνη όταν το κιόσκι είναι ενεργό.",
    },
    "hr": {
        "LOCALE_REGION": "Jezik, regija i vremenska zona",
        "COUNTRY_DOC": "Postavlja regulatornu Wi‑Fi domenu (iw reg) za zemlju korištenja.",
        "KIOSK_WPE": "Kiosk i zaslon (WPE)",
        "KIOSK_WPE_DOC": "Rezervirano za Wayland/WPE ljusku na uređaju; vidi docs/KIOSK.md.",
        "KIOSK_ENABLE": "Omogući kiosk sučelje",
        "PRIMARY_DISPLAY": "Primarni zaslon",
        "PRIMARY_DISPLAY_DOC": "Koji izlaz prikazuje sučelje preko cijelog zaslona kad je kiosk uključen.",
    },
    "hu": {
        "LOCALE_REGION": "Nyelv, régió és időzóna",
        "COUNTRY_DOC": "Beállítja a Wi‑Fi szabályozási tartományt (iw reg) a használat helye szerint.",
        "KIOSK_WPE": "Kioszk és kijelző (WPE)",
        "KIOSK_WPE_DOC": "Fenntartva az eszköz Wayland/WPE héjához; lásd docs/KIOSK.md.",
        "KIOSK_ENABLE": "Kioszk felület engedélyezése",
        "PRIMARY_DISPLAY": "Elsődleges kijelző",
        "PRIMARY_DISPLAY_DOC": "Melyik kimenet mutatja a teljes képernyős felületet kioszk módban.",
    },
    "it": {
        "LOCALE_REGION": "Lingua, regione e fuso orario",
        "COUNTRY_DOC": "Imposta il dominio normativo Wi‑Fi (iw reg) in base al Paese di utilizzo.",
        "KIOSK_WPE": "Chiosco e display (WPE)",
        "KIOSK_WPE_DOC": "Riservato alla shell Wayland/WPE sul dispositivo; vedere docs/KIOSK.md.",
        "KIOSK_ENABLE": "Abilita interfaccia chiosco",
        "PRIMARY_DISPLAY": "Display principale",
        "PRIMARY_DISPLAY_DOC": "Quale uscita mostra l'interfaccia a schermo intero con il chiosco attivo.",
    },
    "ja": {
        "LOCALE_REGION": "言語・地域・タイムゾーン",
        "COUNTRY_DOC": "使用地域に応じて Wi‑Fi の規制ドメイン (iw reg) を設定します。",
        "KIOSK_WPE": "キオスクとディスプレイ (WPE)",
        "KIOSK_WPE_DOC": "オンデバイスの Wayland/WPE シェル用 (詳細は docs/KIOSK.md)。",
        "KIOSK_ENABLE": "キオスク UI を有効にする",
        "PRIMARY_DISPLAY": "プライマリディスプレイ",
        "PRIMARY_DISPLAY_DOC": "キオスク有効時に全画面 UI を表示する出力。",
    },
    "ko": {
        "LOCALE_REGION": "언어, 국가 및 표준 시간대",
        "COUNTRY_DOC": "사용 국가에 따라 Wi‑Fi 규제 도메인(iw reg)을 설정합니다.",
        "KIOSK_WPE": "키오스크 및 디스플레이 (WPE)",
        "KIOSK_WPE_DOC": "기기의 Wayland/WPE 셸용 예약됨. docs/KIOSK.md 참고.",
        "KIOSK_ENABLE": "키오스크 UI 사용",
        "PRIMARY_DISPLAY": "기본 디스플레이",
        "PRIMARY_DISPLAY_DOC": "키오스크가 켜졌을 때 전체 화면 UI를 표시하는 출력입니다.",
    },
    "lt": {
        "LOCALE_REGION": "Kalba, regionas ir laiko juosta",
        "COUNTRY_DOC": "Nustato Wi‑Fi reguliavimo domeną (iw reg) pagal naudojimo šalį.",
        "KIOSK_WPE": "Kioskas ir ekranas (WPE)",
        "KIOSK_WPE_DOC": "Skirta Wayland/WPE apvalkalui įrenginyje; žr. docs/KIOSK.md.",
        "KIOSK_ENABLE": "Įjungti kiosko sąsają",
        "PRIMARY_DISPLAY": "Pagrindinis ekranas",
        "PRIMARY_DISPLAY_DOC": "Kuris išėjimas rodo viso ekrano sąsają įjungus kioską.",
    },
    "nl": {
        "LOCALE_REGION": "Taal, regio en tijdzone",
        "COUNTRY_DOC": "Stelt het Wi‑Fi-regulatory-domein in (iw reg) voor het gebruiksland.",
        "KIOSK_WPE": "Kiosk en scherm (WPE)",
        "KIOSK_WPE_DOC": "Gereserveerd voor Wayland/WPE-shell op het apparaat; zie docs/KIOSK.md.",
        "KIOSK_ENABLE": "Kiosk-ui inschakelen",
        "PRIMARY_DISPLAY": "Primair scherm",
        "PRIMARY_DISPLAY_DOC": "Welke uitgang toont de volledige scherm-interface als kiosk actief is.",
    },
    "no": {
        "LOCALE_REGION": "Språk, region og tidssone",
        "COUNTRY_DOC": "Setter Wi‑Fi-regulatorisk domene (iw reg) for bruksstedet.",
        "KIOSK_WPE": "Kiosk og skjerm (WPE)",
        "KIOSK_WPE_DOC": "Reservert til Wayland/WPE-skall på enheten; se docs/KIOSK.md.",
        "KIOSK_ENABLE": "Aktiver kiosk-grensesnitt",
        "PRIMARY_DISPLAY": "Primær skjerm",
        "PRIMARY_DISPLAY_DOC": "Hvilken utgang viser fullskjerms-UI når kiosk er på.",
    },
    "pl": {
        "LOCALE_REGION": "Język, region i strefa czasowa",
        "COUNTRY_DOC": "Ustawia domenę regulacyjną Wi‑Fi (iw reg) dla kraju użytkowania.",
        "KIOSK_WPE": "Kiosk i wyświetlacz (WPE)",
        "KIOSK_WPE_DOC": "Zarezerwowane dla powłoki Wayland/WPE na urządzeniu; zob. docs/KIOSK.md.",
        "KIOSK_ENABLE": "Włącz interfejs kiosku",
        "PRIMARY_DISPLAY": "Główny ekran",
        "PRIMARY_DISPLAY_DOC": "Które wyjście pokazuje pełnoekranowy interfejs przy włączonym kiosku.",
    },
    "pt": {
        "LOCALE_REGION": "Idioma, região e fuso horário",
        "COUNTRY_DOC": "Define o domínio regulatório Wi‑Fi (iw reg) conforme o país de utilização.",
        "KIOSK_WPE": "Quiosque e ecrã (WPE)",
        "KIOSK_WPE_DOC": "Reservado à shell Wayland/WPE no dispositivo; ver docs/KIOSK.md.",
        "KIOSK_ENABLE": "Ativar interface de quiosque",
        "PRIMARY_DISPLAY": "Ecrã principal",
        "PRIMARY_DISPLAY_DOC": "Qual saída mostra a interface em ecrã inteiro com o quiosque activo.",
    },
    "ru": {
        "LOCALE_REGION": "Язык, регион и часовой пояс",
        "COUNTRY_DOC": "Задаёт регуляторный домен Wi‑Fi (iw reg) для страны использования.",
        "KIOSK_WPE": "Киоск и дисплей (WPE)",
        "KIOSK_WPE_DOC": "Зарезервировано для Wayland/WPE на устройстве; см. docs/KIOSK.md.",
        "KIOSK_ENABLE": "Включить интерфейс киоска",
        "PRIMARY_DISPLAY": "Основной дисплей",
        "PRIMARY_DISPLAY_DOC": "Какой вывод показывает полноэкранный интерфейс при включённом киоске.",
    },
    "si": {
        "LOCALE_REGION": "භාෂාව, කලාපය සහ කාල කලාපය",
        "COUNTRY_DOC": "භාවිතයේ ප්‍රදේශය අනුව Wi‑Fi නියාමන වසම (iw reg) සකසයි.",
        "KIOSK_WPE": "කියෝස්ක හා සංදර්ශකය (WPE)",
        "KIOSK_WPE_DOC": "උපාංගයේ Wayland/WPE කවචය සඳහා වෙන් කර ඇත; බලන්න docs/KIOSK.md.",
        "KIOSK_ENABLE": "කියෝස්ක UI සක්‍රීය කරන්න",
        "PRIMARY_DISPLAY": "ප්‍රාථමික සංදර්ශකය",
        "PRIMARY_DISPLAY_DOC": "කියෝස්ක සක්‍රිය විට පුර්ණතිර අතුරුමුහුණත් පෙන්වන්නේ කුමන ප්‍රදානයද.",
    },
    "sk": {
        "LOCALE_REGION": "Jazyk, región a časové pásmo",
        "COUNTRY_DOC": "Nastaví regulačnú doménu Wi‑Fi (iw reg) podľa krajiny používania.",
        "KIOSK_WPE": "Kiosk a displej (WPE)",
        "KIOSK_WPE_DOC": "Vyhradené pre Wayland/WPE shell na zariadení; pozri docs/KIOSK.md.",
        "KIOSK_ENABLE": "Zapnúť kiosk rozhranie",
        "PRIMARY_DISPLAY": "Primárny displej",
        "PRIMARY_DISPLAY_DOC": "Ktorý výstup zobrazuje rozhranie na celú obrazovku pri zapnutom kiosku.",
    },
    "sr": {
        "LOCALE_REGION": "Jezik, region i vremenska zona",
        "COUNTRY_DOC": "Postavlja regulatorski Wi‑Fi domen (iw reg) za zemlju korišćenja.",
        "KIOSK_WPE": "Kiosk i ekran (WPE)",
        "KIOSK_WPE_DOC": "Rezervisano za Wayland/WPE ljusku na uređaju; vidi docs/KIOSK.md.",
        "KIOSK_ENABLE": "Uključi kiosk interfejs",
        "PRIMARY_DISPLAY": "Primarni ekran",
        "PRIMARY_DISPLAY_DOC": "Koji izlaz prikazuje full screen interfejs kad je kiosk uključen.",
    },
    "sv": {
        "LOCALE_REGION": "Språk, region och tidszon",
        "COUNTRY_DOC": "Anger Wi‑Fi‑regulatorisk domän (iw reg) för användningslandet.",
        "KIOSK_WPE": "Kiosk och skärm (WPE)",
        "KIOSK_WPE_DOC": "Reserverat för Wayland/WPE‑skal på enheten; se docs/KIOSK.md.",
        "KIOSK_ENABLE": "Aktivera kiosk‑gränssnitt",
        "PRIMARY_DISPLAY": "Primär skärm",
        "PRIMARY_DISPLAY_DOC": "Vilken utgång visar helskärms‑UI när kiosk är aktiv.",
    },
    "th": {
        "LOCALE_REGION": "ภาษา ภูมิภาค และเขตเวลา",
        "COUNTRY_DOC": "ตั้งค่าโดเมนกำกับ Wi‑Fi (iw reg) ตามประเทศที่ใช้งาน",
        "KIOSK_WPE": "คีออสก์และจอแสดงผล (WPE)",
        "KIOSK_WPE_DOC": "สำรองสำหรับ Wayland/WPE shell บนอุปกรณ์ ดู docs/KIOSK.md",
        "KIOSK_ENABLE": "เปิดใช้อินเทอร์เฟซคีออสก์",
        "PRIMARY_DISPLAY": "จอหลัก",
        "PRIMARY_DISPLAY_DOC": "เอาต์พุตใดแสดง UI เต็มจอเมื่อเปิดโหมดคีออสก์",
    },
    "tr": {
        "LOCALE_REGION": "Dil, bölge ve saat dilimi",
        "COUNTRY_DOC": "Kullanım ülkesine göre Wi‑Fi düzenleme alanını (iw reg) ayarlar.",
        "KIOSK_WPE": "Kiosk ve ekran (WPE)",
        "KIOSK_WPE_DOC": "Cihaz üzerinde Wayland/WPE kabuğu için ayrılmıştır; bkz. docs/KIOSK.md.",
        "KIOSK_ENABLE": "Kiosk arayüzünü etkinleştir",
        "PRIMARY_DISPLAY": "Birincil ekran",
        "PRIMARY_DISPLAY_DOC": "Kiosk açıkken tam ekran arayüzü hangi çıkış gösterir.",
    },
    "ua": {
        "LOCALE_REGION": "Мова, регіон і часовий пояс",
        "COUNTRY_DOC": "Задає регуляторний домен Wi‑Fi (iw reg) для країни використання.",
        "KIOSK_WPE": "Кіоск і дисплей (WPE)",
        "KIOSK_WPE_DOC": "Зарезервовано для Wayland/WPE на пристрої; див. docs/KIOSK.md.",
        "KIOSK_ENABLE": "Увімкнути інтерфейс кіоску",
        "PRIMARY_DISPLAY": "Основний дисплей",
        "PRIMARY_DISPLAY_DOC": "Який вивід показує повноекранний інтерфейс за увімкненого кіоску.",
    },
    "vi": {
        "LOCALE_REGION": "Ngôn ngữ, khu vực và múi giờ",
        "COUNTRY_DOC": "Đặt miền quy định Wi‑Fi (iw reg) theo quốc gia sử dụng.",
        "KIOSK_WPE": "Kiosk và màn hình (WPE)",
        "KIOSK_WPE_DOC": "Dành cho shell Wayland/WPE trên thiết bị; xem docs/KIOSK.md.",
        "KIOSK_ENABLE": "Bật giao diện kiosk",
        "PRIMARY_DISPLAY": "Màn hình chính",
        "PRIMARY_DISPLAY_DOC": "Đầu ra nào hiển thị giao diện toàn màn hình khi bật kiosk.",
    },
    "zh": {
        "LOCALE_REGION": "语言、地区与时区",
        "COUNTRY_DOC": "根据使用地区设置 Wi‑Fi 管制域（iw reg）。",
        "KIOSK_WPE": "展台与显示器（WPE）",
        "KIOSK_WPE_DOC": "预留给设备上的 Wayland/WPE 壳层；见 docs/KIOSK.md。",
        "KIOSK_ENABLE": "启用展台界面",
        "PRIMARY_DISPLAY": "主显示器",
        "PRIMARY_DISPLAY_DOC": "开启展台时由哪个输出显示全屏界面。",
    },
    "zh_TW": {
        "LOCALE_REGION": "語言、地區與時區",
        "COUNTRY_DOC": "依使用地區設定 Wi‑Fi 管制網域（iw reg）。",
        "KIOSK_WPE": "展場與顯示器（WPE）",
        "KIOSK_WPE_DOC": "預留給裝置上的 Wayland/WPE 殼層；見 docs/KIOSK.md。",
        "KIOSK_ENABLE": "啟用展場介面",
        "PRIMARY_DISPLAY": "主顯示器",
        "PRIMARY_DISPLAY_DOC": "展場開啟時由哪個輸出顯示全螢幕介面。",
    },
}


def locale_code_from_filename(name: str) -> str | None:
    if not name.startswith("strings_") or not name.endswith(".json"):
        return None
    if name == "strings_en.json":
        return None
    return name[len("strings_") : -len(".json")]


def main() -> None:
    missing = []
    for theme_dir in THEMES:
        if not theme_dir.is_dir():
            print("skip missing dir", theme_dir)
            continue
        for fp in sorted(theme_dir.glob("strings_*.json")):
            code = locale_code_from_filename(fp.name)
            if code is None:
                continue
            patch = TRANSLATIONS.get(code)
            if patch is None:
                missing.append((fp, code))
                continue
            data = json.loads(fp.read_text(encoding="utf-8"))
            sys_o = data.setdefault("SYSTEM", {})
            for k, v in patch.items():
                sys_o[k] = v
            fp.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
            print("updated", fp.relative_to(ROOT))
    if missing:
        print("\nMissing translations for codes:", sorted({c for _, c in missing}))
        raise SystemExit(1)


if __name__ == "__main__":
    main()
