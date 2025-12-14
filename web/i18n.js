/**
 * Simple i18n module for language support
 */

class I18n {
  constructor() {
    this.translations = {};
    this.currentLang = this.detectLanguage();
    this.defaultLang = 'en';
    this.availableLanguages = ['en', 'de'];
  }

  /**
   * Detects the user's preferred language
   * Priority: localStorage > browser language > default (en)
   */
  detectLanguage() {
    // Check localStorage first
    const stored = localStorage.getItem('language');
    if (stored && this.availableLanguages.includes(stored)) {
      return stored;
    }

    // Check browser language
    const browserLang = navigator.language.split('-')[0]; // e.g., 'de-DE' -> 'de'
    if (this.availableLanguages.includes(browserLang)) {
      return browserLang;
    }

    // Default to English
    return 'en';
  }

  /**
   * Loads translation file for specified language
   */
  async load(lang) {
    if (!this.availableLanguages.includes(lang)) {
      console.warn(`Language ${lang} not available, falling back to ${this.defaultLang}`);
      lang = this.defaultLang;
    }

    try {
      const response = await fetch(`./i18n/${lang}.json`);
      if (!response.ok) {
        throw new Error(`Failed to load ${lang}.json`);
      }
      this.translations[lang] = await response.json();
      this.currentLang = lang;
      localStorage.setItem('language', lang);
      this.updatePageLanguage();
      return true;
    } catch (error) {
      console.error(`Error loading language ${lang}:`, error);
      if (lang !== this.defaultLang) {
        // Fallback to default language
        return this.load(this.defaultLang);
      }
      return false;
    }
  }

  /**
   * Updates the HTML lang attribute
   */
  updatePageLanguage() {
    document.documentElement.lang = this.currentLang;
  }

  /**
   * Gets a translation by key path (e.g., 'stats.title')
   * @param {string} key - Dot-notation path to translation
   * @param {object} params - Optional parameters for string interpolation
   * @returns {string} - Translated string or key if not found
   */
  t(key, params = {}) {
    const lang = this.translations[this.currentLang];
    if (!lang) {
      console.warn(`Translations not loaded for ${this.currentLang}`);
      return key;
    }

    const keys = key.split('.');
    let value = lang;

    for (const k of keys) {
      if (value && typeof value === 'object' && k in value) {
        value = value[k];
      } else {
        console.warn(`Translation key not found: ${key}`);
        return key;
      }
    }

    // Simple parameter replacement {param}
    if (typeof value === 'string' && Object.keys(params).length > 0) {
      return value.replace(/\{(\w+)\}/g, (match, param) => {
        return params[param] !== undefined ? params[param] : match;
      });
    }

    return value;
  }

  /**
   * Changes the current language
   */
  async setLanguage(lang) {
    if (lang === this.currentLang && this.translations[lang]) {
      return true;
    }
    return await this.load(lang);
  }

  /**
   * Gets current language code
   */
  getLanguage() {
    return this.currentLang;
  }

  /**
   * Gets list of available languages
   */
  getAvailableLanguages() {
    return this.availableLanguages;
  }
}

// Create and export singleton instance
const i18n = new I18n();
export default i18n;
