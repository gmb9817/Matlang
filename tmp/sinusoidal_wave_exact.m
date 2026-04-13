x = linspace(0, 2*pi, 100);
y = sin(x);
figure;
plot(x, y, 'LineWidth', 2);
title('Sinusoidal Wave');
xlabel('x');
ylabel('sin(x)');
grid on;
