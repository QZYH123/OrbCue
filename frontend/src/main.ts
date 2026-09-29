import { mount } from 'svelte';
import './app.css';
import { applyDocumentLang } from './locale';
import { initTheme } from './theme';
import App from './App.svelte';

applyDocumentLang();
initTheme();

const app = mount(App, { target: document.getElementById('app')! });

export default app;

