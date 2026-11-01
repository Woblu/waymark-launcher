import { mount } from 'svelte';
import '@fontsource-variable/libre-franklin/index.css';
import '@fontsource-variable/libre-franklin/wght-italic.css';
import './styles/tokens.css';
import './styles/base.css';
import App from './App.svelte';
import { startTranslation } from './lib/i18n';
import { showTheme } from './lib/theme';

showTheme();

mount(App, { target: document.getElementById('app')! });
startTranslation();
