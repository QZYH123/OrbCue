import { mount } from 'svelte';
import './app.css';
import { initTheme } from './theme';
import App from './App.svelte';

initTheme();

const app = mount(App, { target: document.getElementById('app')! });

export default app;

