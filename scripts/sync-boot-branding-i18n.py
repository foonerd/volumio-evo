#!/usr/bin/env python3
"""Insert SYSTEM.BOOT_BRANDING* keys into every strings_*.json when missing (idempotent)."""
from __future__ import annotations

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "layer" / "web"

EN = {
    "BOOT_BRANDING": "Boot branding",
    "BOOT_BRANDING_ROTATION": "Splash rotation",
    "BOOT_BRANDING_ROTATION_DOC": (
        "Rotation hint for Plymouth assets (kernel parameter plymouth=N). "
        "Match your display orientation."
    ),
    "INSTALL_BOOT_BRANDING": "Install and enable",
}

# Language code = filename stem after "strings_" (e.g. zh_TW).
TRANSLATIONS: dict[str, dict[str, str]] = {
    "ca": {
        "BOOT_BRANDING": "Marca d'arrencada",
        "BOOT_BRANDING_ROTATION": "Rotació de la pantalla inicial",
        "BOOT_BRANDING_ROTATION_DOC": (
            "Indicació de rotació per als recursos Plymouth (paràmetre plymouth=N del nucli). "
            "Alineu amb l'orientació de la pantalla."
        ),
        "INSTALL_BOOT_BRANDING": "Instal·lar i activar",
    },
    "cs": {
        "BOOT_BRANDING": "Značka při bootu",
        "BOOT_BRANDING_ROTATION": "Rotace splash",
        "BOOT_BRANDING_ROTATION_DOC": (
            "Nápověda rotace pro assety Plymouth (parametr jádra plymouth=N). "
            "Přiměřte orientaci displeje."
        ),
        "INSTALL_BOOT_BRANDING": "Nainstalovat a povolit",
    },
    "da": {
        "BOOT_BRANDING": "Boot-branding",
        "BOOT_BRANDING_ROTATION": "Splash-rotation",
        "BOOT_BRANDING_ROTATION_DOC": (
            "Rotationshint til Plymouth-aktiver (kerneparameter plymouth=N). "
            "Tilpas skærmens orientering."
        ),
        "INSTALL_BOOT_BRANDING": "Installer og aktiver",
    },
    "de": {
        "BOOT_BRANDING": "Boot-Branding",
        "BOOT_BRANDING_ROTATION": "Splash-Rotation",
        "BOOT_BRANDING_ROTATION_DOC": (
            "Drehhinweis für Plymouth-Assets (Kernel-Parameter plymouth=N). "
            "An die Displayausrichtung anpassen."
        ),
        "INSTALL_BOOT_BRANDING": "Installieren und aktivieren",
    },
    "es": {
        "BOOT_BRANDING": "Marca de arranque",
        "BOOT_BRANDING_ROTATION": "Rotación del splash",
        "BOOT_BRANDING_ROTATION_DOC": (
            "Indicación de rotación para los recursos Plymouth (parámetro plymouth=N). "
            "Iguala la orientación de la pantalla."
        ),
        "INSTALL_BOOT_BRANDING": "Instalar y activar",
    },
    "fi": {
        "BOOT_BRANDING": "Käynnistyksen brändäys",
        "BOOT_BRANDING_ROTATION": "Splash-kierto",
        "BOOT_BRANDING_ROTATION_DOC": (
            "Kiertovihje Plymouth-resursseille (kernel-parametri plymouth=N). "
            "Sovita näytön suuntaan."
        ),
        "INSTALL_BOOT_BRANDING": "Asenna ja ota käyttöön",
    },
    "fr": {
        "BOOT_BRANDING": "Écran de démarrage",
        "BOOT_BRANDING_ROTATION": "Rotation du splash",
        "BOOT_BRANDING_ROTATION_DOC": (
            "Indication de rotation pour les ressources Plymouth (paramètre plymouth=N). "
            "À faire correspondre à l’orientation de l’écran."
        ),
        "INSTALL_BOOT_BRANDING": "Installer et activer",
    },
    "gr": {
        "BOOT_BRANDING": "Επωνυμία εκκίνησης",
        "BOOT_BRANDING_ROTATION": "Περιστροφή splash",
        "BOOT_BRANDING_ROTATION_DOC": (
            "Υπόδειξη περιστροφής για τα στοιχεία Plymouth (παράμετρος plymouth=N). "
            "Ταιριάξτε με τον προσανατολισμό της οθόνης."
        ),
        "INSTALL_BOOT_BRANDING": "Εγκατάσταση και ενεργοποίηση",
    },
    "hr": {
        "BOOT_BRANDING": "Boot branding",
        "BOOT_BRANDING_ROTATION": "Rotacija splasha",
        "BOOT_BRANDING_ROTATION_DOC": (
            "Rotacijski savjet za Plymouth resurse (parametar jezgra plymouth=N). "
            "Uskladite s orijentacijom zaslona."
        ),
        "INSTALL_BOOT_BRANDING": "Instaliraj i omogući",
    },
    "hu": {
        "BOOT_BRANDING": "Boot branding",
        "BOOT_BRANDING_ROTATION": "Splash elforgatás",
        "BOOT_BRANDING_ROTATION_DOC": (
            "Forgatási jelzés Plymouth elemekhez (kernel plymouth=N). "
            "Igazítsa a kijelző tájolásához."
        ),
        "INSTALL_BOOT_BRANDING": "Telepítés és engedélyezés",
    },
    "it": {
        "BOOT_BRANDING": "Branding di avvio",
        "BOOT_BRANDING_ROTATION": "Rotazione splash",
        "BOOT_BRANDING_ROTATION_DOC": (
            "Suggerimento di rotazione per le risorse Plymouth (parametro plymouth=N). "
            "Allinea l’orientamento del display."
        ),
        "INSTALL_BOOT_BRANDING": "Installa e abilita",
    },
    "ja": {
        "BOOT_BRANDING": "起動ブランディング",
        "BOOT_BRANDING_ROTATION": "スプラッシュの向き",
        "BOOT_BRANDING_ROTATION_DOC": (
            "Plymouth アセットの回転ヒント（カーネルパラメータ plymouth=N）。"
            "ディスプレイの向きに合わせます。"
        ),
        "INSTALL_BOOT_BRANDING": "インストールして有効化",
    },
    "ko": {
        "BOOT_BRANDING": "부트 브랜딩",
        "BOOT_BRANDING_ROTATION": "스플래시 회전",
        "BOOT_BRANDING_ROTATION_DOC": (
            "Plymouth 리소스 회전 안내(커널 plymouth=N). 디스플레이 방향에 맞추세요."
        ),
        "INSTALL_BOOT_BRANDING": "설치 및 사용",
    },
    "lt": {
        "BOOT_BRANDING": "Paleidimo ženklinimas",
        "BOOT_BRANDING_ROTATION": "Splash pasukimas",
        "BOOT_BRANDING_ROTATION_DOC": (
            "Pasukimo užuomina Plymouth ištekliams (branduolio plymouth=N). "
            "Derinkite su ekrano orientacija."
        ),
        "INSTALL_BOOT_BRANDING": "Įdiegti ir įjungti",
    },
    "nl": {
        "BOOT_BRANDING": "Opstartbranding",
        "BOOT_BRANDING_ROTATION": "Splash-rotatie",
        "BOOT_BRANDING_ROTATION_DOC": (
            "Rotatiehint voor Plymouth-assets (kernelparameter plymouth=N). "
            "Stem af op schermoriëntatie."
        ),
        "INSTALL_BOOT_BRANDING": "Installeren en inschakelen",
    },
    "no": {
        "BOOT_BRANDING": "Oppstartsbranding",
        "BOOT_BRANDING_ROTATION": "Splash-rotasjon",
        "BOOT_BRANDING_ROTATION_DOC": (
            "Rotasjonshint for Plymouth-ressurser (kjerneparameter plymouth=N). "
            "Tilpass skjermretningen."
        ),
        "INSTALL_BOOT_BRANDING": "Installer og aktiver",
    },
    "pl": {
        "BOOT_BRANDING": "Oznakowanie rozruchu",
        "BOOT_BRANDING_ROTATION": "Obrót splash",
        "BOOT_BRANDING_ROTATION_DOC": (
            "Wskazówka obrotu dla zasobów Plymouth (parametr plymouth=N). "
            "Dopasuj orientację wyświetlacza."
        ),
        "INSTALL_BOOT_BRANDING": "Zainstaluj i włącz",
    },
    "pt": {
        "BOOT_BRANDING": "Marca de arranque",
        "BOOT_BRANDING_ROTATION": "Rotação do splash",
        "BOOT_BRANDING_ROTATION_DOC": (
            "Dica de rotação para recursos Plymouth (parâmetro plymouth=N). "
            "Corresponda à orientação do ecrã."
        ),
        "INSTALL_BOOT_BRANDING": "Instalar e ativar",
    },
    "ru": {
        "BOOT_BRANDING": "Заставка при загрузке",
        "BOOT_BRANDING_ROTATION": "Поворот заставки",
        "BOOT_BRANDING_ROTATION_DOC": (
            "Подсказка поворота для ресурсов Plymouth (параметр plymouth=N). "
            "Соответствует ориентации дисплея."
        ),
        "INSTALL_BOOT_BRANDING": "Установить и включить",
    },
    "si": {
        "BOOT_BRANDING": "Boot branding",
        "BOOT_BRANDING_ROTATION": "Rotacija začetnega zaslona",
        "BOOT_BRANDING_ROTATION_DOC": (
            "Namig za rotacijo sredstev Plymouth (jedro plymouth=N). "
            "Ujemanje z orientacijo zaslona."
        ),
        "INSTALL_BOOT_BRANDING": "Namesti in omogoči",
    },
    "sk": {
        "BOOT_BRANDING": "Boot branding",
        "BOOT_BRANDING_ROTATION": "Otáčanie splash",
        "BOOT_BRANDING_ROTATION_DOC": (
            "Nápoveda rotácie pre Plymouth (parameter plymouth=N). "
            "Zlaďte s orientáciou displeja."
        ),
        "INSTALL_BOOT_BRANDING": "Nainštalovať a povoliť",
    },
    "sr": {
        "BOOT_BRANDING": "Boot branding",
        "BOOT_BRANDING_ROTATION": "Ротација splash-а",
        "BOOT_BRANDING_ROTATION_DOC": (
            "Савет за ротацију Plymouth ресурса (параметар plymouth=N). "
            "Ускладите са оријентацијом екрана."
        ),
        "INSTALL_BOOT_BRANDING": "Инсталирај и омогући",
    },
    "sv": {
        "BOOT_BRANDING": "Boot-branding",
        "BOOT_BRANDING_ROTATION": "Splash-rotation",
        "BOOT_BRANDING_ROTATION_DOC": (
            "Rotationshint för Plymouth-resurser (kärnparameter plymouth=N). "
            "Matcha skärmorientering."
        ),
        "INSTALL_BOOT_BRANDING": "Installera och aktivera",
    },
    "th": {
        "BOOT_BRANDING": "แบรนด์ขณะบูต",
        "BOOT_BRANDING_ROTATION": "การหมุนสแปลช",
        "BOOT_BRANDING_ROTATION_DOC": (
            "คำแนะนำการหมุนสำหรับ Plymouth (พารามิเตอร์ plymouth=N) "
            "ให้ตรงกับทิศทางจอ"
        ),
        "INSTALL_BOOT_BRANDING": "ติดตั้งและเปิดใช้งาน",
    },
    "tr": {
        "BOOT_BRANDING": "Önyükleme markalama",
        "BOOT_BRANDING_ROTATION": "Splash dönüşü",
        "BOOT_BRANDING_ROTATION_DOC": (
            "Plymouth öğeleri için dönüş ipucu (çekirdek plymouth=N). "
            "Ekran yönüyle eşleştirin."
        ),
        "INSTALL_BOOT_BRANDING": "Kur ve etkinleştir",
    },
    "ua": {
        "BOOT_BRANDING": "Брендинг завантаження",
        "BOOT_BRANDING_ROTATION": "Обертання заставки",
        "BOOT_BRANDING_ROTATION_DOC": (
            "Підказка обертання для ресурсів Plymouth (параметр plymouth=N). "
            "Відповідає орієнтації дисплея."
        ),
        "INSTALL_BOOT_BRANDING": "Установити й увімкнути",
    },
    "vi": {
        "BOOT_BRANDING": "Branding khi khởi động",
        "BOOT_BRANDING_ROTATION": "Xoay splash",
        "BOOT_BRANDING_ROTATION_DOC": (
            "Gợi ý xoay cho tài nguyên Plymouth (tham số plymouth=N). "
            "Khớp hướng màn hình."
        ),
        "INSTALL_BOOT_BRANDING": "Cài đặt và bật",
    },
    "zh": {
        "BOOT_BRANDING": "启动画面品牌",
        "BOOT_BRANDING_ROTATION": "开机动画旋转",
        "BOOT_BRANDING_ROTATION_DOC": (
            "Plymouth 资源的旋转提示（内核参数 plymouth=N）。请与显示屏方向一致。"
        ),
        "INSTALL_BOOT_BRANDING": "安装并启用",
    },
    "zh_TW": {
        "BOOT_BRANDING": "開機畫面品牌",
        "BOOT_BRANDING_ROTATION": "開機動畫旋轉",
        "BOOT_BRANDING_ROTATION_DOC": (
            "Plymouth 資源的旋轉提示（核心參數 plymouth=N）。請配合螢幕方向。"
        ),
        "INSTALL_BOOT_BRANDING": "安裝並啟用",
    },
}


def lang_code(path: Path) -> str:
    return path.stem.removeprefix("strings_")


def merge_strings(lang: str) -> dict[str, str]:
    base = TRANSLATIONS.get(lang, EN).copy()
    # Ensure every required key exists
    for k, v in EN.items():
        base.setdefault(k, v)
    return base


def patch(path: Path) -> bool:
    raw = path.read_text(encoding="utf-8")
    data = json.loads(raw)
    sysd = data.get("SYSTEM")
    if not isinstance(sysd, dict):
        print(f"skip (no SYSTEM): {path}", file=sys.stderr)
        return False
    if "BOOT_BRANDING" in sysd:
        return False
    lang = lang_code(path)
    extra = merge_strings(lang)
    new_sys: dict = dict(sysd)
    if "ALLOW_UI_STATISTICS_DOC" in new_sys:
        merged: dict = {}
        for k, v in new_sys.items():
            merged[k] = v
            if k == "ALLOW_UI_STATISTICS_DOC":
                for ek, ev in extra.items():
                    merged[ek] = ev
        new_sys = merged
    else:
        # Stub locales (e.g. si/sr) with minimal SYSTEM — append keys at end.
        for ek, ev in extra.items():
            new_sys[ek] = ev
    data["SYSTEM"] = new_sys
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return True


def main() -> int:
    if not ROOT.is_dir():
        print(f"Missing {ROOT}", file=sys.stderr)
        return 1
    n = 0
    for path in sorted(ROOT.rglob("strings_*.json")):
        if patch(path):
            print(path)
            n += 1
    print(f"Patched {n} files.", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
