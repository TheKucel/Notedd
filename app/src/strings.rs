//! App labels in English, Russian and Simplified Chinese, in that order.
use crate::locale::Message;

pub const SETTINGS: Message = Message("Settings", "Настройки", "设置");
pub const GENERAL: Message = Message("General", "Общие", "常规");
pub const SHORTCUTS: Message = Message("Shortcuts", "Сочетания клавиш", "快捷键");
pub const COLORS: Message = Message("Colors", "Цвета", "颜色");
pub const LANGUAGE: Message = Message("Language", "Язык", "语言");
pub const EDITOR: Message = Message("Editor", "Редактор", "编辑器");
pub const SIMPLE: Message = Message("Simple", "Простой", "简洁");
pub const BOLD: Message = Message("Bold", "Жирный", "加粗");
pub const ITALIC: Message = Message("Italic", "Курсив", "斜体");
pub const STRIKE: Message = Message("Strikethrough", "Зачёркнутый", "删除线");
pub const CODE: Message = Message("Inline code", "Код", "行内代码");
pub const UNDO: Message = Message("Undo", "Отменить", "撤销");
pub const CUT: Message = Message("Cut", "Вырезать", "剪切");
pub const COPY: Message = Message("Copy", "Копировать", "复制");
pub const PASTE: Message = Message("Paste", "Вставить", "粘贴");
pub const SELECT_ALL: Message = Message("Select all", "Выделить всё", "全选");
pub const SYSTEM_LANGUAGE: Message = Message("System", "Как в системе", "跟随系统");
pub const THEME: Message = Message("Theme", "Тема", "主题");
pub const PAPER: Message = Message("Paper", "Бумага", "纸张");
pub const NIGHT: Message = Message("Night", "Ночь", "夜间");
pub const SYSTEM_THEME: Message = Message("System", "Как в системе", "跟随系统");
pub const CUSTOM_THEME: Message = Message("Custom", "Своя", "自定义");
pub const TEXT_SIZE: Message = Message("Text size", "Размер текста", "文字大小");
pub const APPEARANCE: Message = Message("Show from", "Появление", "弹出方向");
pub const BOTTOM: Message = Message("Bottom", "Снизу", "下方");
pub const RIGHT: Message = Message("Right", "Справа", "右侧");
pub const LEFT: Message = Message("Left", "Слева", "左侧");
pub const TOP: Message = Message("Top", "Сверху", "上方");
pub const IN_PLACE: Message = Message("In place", "На месте", "原位");
pub const SLIDE_DISTANCE: Message = Message("Slide distance", "Откуда", "滑动距离");
pub const NEAR: Message = Message("Nearby", "Рядом", "附近");
pub const OFF_SCREEN: Message = Message("Off-screen", "Из-за края экрана", "屏幕外");
pub const SPEED: Message = Message("Speed", "Скорость", "速度");
pub const FAST: Message = Message("Fast", "Быстро", "快速");
pub const NORMAL: Message = Message("Normal", "Обычно", "普通");
pub const SLOW: Message = Message("Slow", "Плавно", "缓慢");
pub const START_WITH_WINDOWS: Message = Message("Start with system", "Запуск вместе с системой", "随系统启动");
pub const ON: Message = Message("On", "Вкл", "开启");
pub const OFF: Message = Message("Off", "Выкл", "关闭");
pub const NOTES_FOLDER: Message = Message("Notes folder", "Папка заметок", "笔记文件夹");
pub const OPEN: Message = Message("Open", "Открыть", "打开");
pub const MOVE: Message = Message("Move…", "Перенести…", "移动…");
pub const MOVE_NOTES: Message = Message("Choose where to move your notes", "Куда перенести заметки", "选择笔记的新位置");
pub const MOVE_FAILED: Message = Message("Couldn't move notes", "Не удалось перенести заметки", "无法移动笔记");
pub const TEXT: Message = Message("Text", "Текст", "文字");
pub const ACCENT: Message = Message("Accent", "Акцент", "强调色");
pub const CUSTOM_COLOR: Message = Message("Custom", "Свой", "自定义");
pub const FROM_THEME: Message = Message("From theme", "Из темы", "使用主题颜色");
pub const CARDS: Message = Message("Cards", "Карточки", "卡片");
pub const RESET_COLORS: Message = Message("Reset theme colors", "Сбросить цвета этой темы", "重置主题颜色");
pub const COLOR_CODE: Message = Message("Enter a hex code", "Введите код", "输入十六进制色值");
pub const COLOR_DONE: Message = Message("Esc: Done", "Esc: готово", "Esc：完成");
pub const COLOR_KEYS: Message = Message("←→ Hue · Ctrl+←→ Saturation · Ctrl+↑↓ Brightness", "←→ оттенок · Ctrl+←→ насыщенность · Ctrl+↑↓ яркость", "←→ 色相 · Ctrl+←→ 饱和度 · Ctrl+↑↓ 亮度");
pub const GENERAL_HELP: Message = Message("Arrows and Tab: navigate · Ctrl+Tab: next page", "Стрелки и Tab: по пунктам · Ctrl+Tab: следующая страница", "方向键和 Tab：导航 · Ctrl+Tab：下一页");
pub const SHORTCUT_HELP: Message = Message("Click a shortcut or press Enter to edit. Backspace clears it. Esc cancels.", "Нажмите на сочетание или Enter и введите новое. Backspace убирает, Esc отменяет.", "点击快捷键或按 Enter 后输入新组合。Backspace 清除，Esc 取消。");
pub const GLOBAL_HELP: Message = Message("Esc hides Notedd · Ctrl+/− or Ctrl+scroll changes text size · Ctrl+0 resets", "Всегда: Esc скрывает окно · Ctrl + и Ctrl − или Ctrl+колесо меняют размер текста · Ctrl+0 возвращает обычный", "Esc 隐藏 Notedd · Ctrl +/− 或 Ctrl+滚轮调整文字大小 · Ctrl+0 重置");
pub const COLOR_HELP: Message = Message("Click a color to change it for the selected theme.", "Нажмите на цвет, чтобы изменить его. Изменения относятся к выбранной теме.", "点击颜色即可更改当前主题的配色。");
pub const PRESS_SHORTCUT: Message = Message("Press a shortcut…", "Нажмите сочетание…", "请按快捷键…");
pub const NOT_SET: Message = Message("Not set", "Не задано", "未设置");
pub const SHORTCUT_TAKEN: Message = Message("In use by another app", "занято другой программой", "已被其他应用占用");

pub const SEARCH_NOTES: Message = Message("Search notes", "Поиск по заметкам", "搜索笔记");
pub const FIND_IN_NOTE: Message = Message("Find in note", "Найти в заметке", "在笔记中查找");
pub const NOTE_DELETED: Message = Message("Note deleted · Ctrl+Z to undo", "Заметка удалена · Ctrl+Z вернёт её", "笔记已删除 · Ctrl+Z 撤销");
pub const DELETE_NOTE: Message = Message("Delete note", "Удалить заметку", "删除笔记");
pub const NO_RESULTS: Message = Message("No matches for “{query}”. Press Enter to create a note.", "По запросу «{query}» ничего нет. Enter создаст такую заметку.", "没有找到“{query}”。按 Enter 新建笔记。");
pub const UNTITLED: Message = Message("Untitled", "Без названия", "无标题");
pub const TODAY: Message = Message("Today", "Сегодня", "今天");
pub const YESTERDAY: Message = Message("Yesterday", "Вчера", "昨天");
pub const SHOW: Message = Message("Show Notedd", "Показать Notedd", "显示 Notedd");
pub const OPEN_NOTES_FOLDER: Message = Message("Open notes folder", "Открыть папку с заметками", "打开笔记文件夹");
pub const QUIT: Message = Message("Quit", "Выход", "退出");

pub const KEY_NAMES: [Message; crate::settings::KEYS] = [
    Message("Show / hide (global)", "Показать / скрыть (везде)", "显示／隐藏（全局）"),
    Message("New note (global)", "Новая заметка (везде)", "新建笔记（全局）"),
    Message("New note", "Новая заметка", "新建笔记"),
    Message("All notes", "Все заметки", "所有笔记"),
    Message("Find in note", "Поиск в заметке", "在笔记中查找"),
    SETTINGS,
    Message("Always on top", "Закрепить поверх окон", "窗口置顶"),
    QUIT,
    Message("Note color", "Цвет заметки", "笔记颜色"),
    DELETE_NOTE,
    Message("Highlight text", "Подсветить текст", "高亮文字"),
];

pub const WELCOME: Message = Message(
    "Notedd\nQuick notes, always at hand. Ctrl+Alt+Space shows Notedd from anywhere. Ctrl+P: all notes, Ctrl+F: find, Ctrl+N: new note.\n- Start a list with ‘- ’\n1. Start a numbered list with ‘1. ’",
    "Notedd\nБыстрые заметки. Ctrl+Alt+Space открывает окно из любого места. Ctrl+P: все заметки, Ctrl+F: поиск, Ctrl+N: новая.\n- список начинается с «- »\n1. а нумерованный с «1. »",
    "Notedd\n随手记下灵感。Ctrl+Alt+Space 可随时打开 Notedd。Ctrl+P 查看所有笔记，Ctrl+F 查找，Ctrl+N 新建笔记。\n- 用“- ”开始项目列表\n1. 用“1. ”开始编号列表",
);
