import { mount } from 'svelte';
import '@fontsource-variable/libre-franklin/index.css';
import './styles/tokens.css';
import './styles/base.css';
import App from './App.svelte';

mount(App, { target: document.getElementById('app')! });
