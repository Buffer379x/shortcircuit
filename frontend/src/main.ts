import { mount } from 'svelte';
import App from './App.svelte';
import './styles/tokens.css';
import './styles/base.css';
import './styles/ui.css';
import './styles/shell.css';

mount(App, { target: document.getElementById('app')! });
