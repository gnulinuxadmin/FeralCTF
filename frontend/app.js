// FeralCTF - Frontend JavaScript
// Stub implementation for Sprint 0
// See FERALCTF_SPRINTS.md for requirements

// DOM Elements
const homeSection = document.getElementById('home');
const challengesSection = document.getElementById('challenges');
const scoreboardSection = document.getElementById('scoreboard');
const loginSection = document.getElementById('login');
const registerSection = document.getElementById('register');

// Navigation
document.querySelectorAll('.nav-links a').forEach(link => {
    link.addEventListener('click', (e) => {
        e.preventDefault();
        const target = e.target.getAttribute('href');
        
        // Hide all sections
        [homeSection, challengesSection, scoreboardSection, loginSection, registerSection].forEach(section => {
            section.style.display = 'none';
        });
        
        // Show target section
        document.querySelector(target).style.display = 'block';
    });
});

// Form submissions
document.getElementById('login-form').addEventListener('submit', (e) => {
    e.preventDefault();
    // Stub implementation
    console.log('Login form submitted');
});

document.getElementById('register-form').addEventListener('submit', (e) => {
    e.preventDefault();
    // Stub implementation
    console.log('Register form submitted');
});

// Load initial data
document.addEventListener('DOMContentLoaded', () => {
    // Show home section by default
    homeSection.style.display = 'block';
    
    // Stub: Load challenges
    loadChallenges();
    
    // Stub: Load scoreboard
    loadScoreboard();
});

function loadChallenges() {
    // Stub implementation
    console.log('Loading challenges...');
}

function loadScoreboard() {
    // Stub implementation
    console.log('Loading scoreboard...');
}