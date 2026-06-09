import React, { useState, useEffect, useRef } from 'react';
import {
  Card,
  Input,
  Button,
  Space,
  Typography,
  Select,
  Row,
  Col,
  Tag,
  Divider,
  message,
  Tooltip,
} from 'antd';
import {
  ConsoleSqlOutlined,
  PlayCircleOutlined,
  ClearOutlined,
  CopyOutlined,
  ReloadOutlined,
  InfoCircleOutlined,
} from '@ant-design/icons';
import { apiPost } from '../utils/api';

const { Title, Text } = Typography;
const { TextArea } = Input;
const { Option } = Select;

const Terminal = () => {
  const [command, setCommand] = useState('');
  const [output, setOutput] = useState([]);
  const [loading, setLoading] = useState(false);
  const [selectedCommand, setSelectedCommand] = useState('');
  const outputRef = useRef(null);

  const predefinedCommands = [
    {
      name: 'System Status',
      command: 'kubectl get pods -n wasmbed',
      description: 'Check all pods in wasmbed namespace',
      category: 'Kubernetes'
    },
    {
      name: 'Device Status',
      command: 'kubectl get devices -n wasmbed',
      description: 'List all devices with status and gateway info',
      category: 'Devices'
    },
    {
      name: 'Application Status',
      command: 'kubectl get applications -n wasmbed',
      description: 'List all applications with deployment status',
      category: 'Applications'
    },
    {
      name: 'Gateway Status',
      command: 'kubectl get gateways -n wasmbed',
      description: 'List all gateways with connection statistics',
      category: 'Gateways'
    },
    {
      name: 'Infrastructure Health',
      command: 'curl -s http://wasmbed-api-server.wasmbed.svc.cluster.local:3001/api/v1/infrastructure/health',
      description: 'Check infrastructure service health status',
      category: 'Infrastructure'
    },
    {
      name: 'Device Details',
      command: 'kubectl describe device -n wasmbed',
      description: 'Get detailed device information and configuration',
      category: 'Devices'
    },
    {
      name: 'Application Logs',
      command: 'kubectl logs -n wasmbed -l app=wasmbed-application-controller',
      description: 'View application controller logs for debugging',
      category: 'Logs'
    },
    {
      name: 'Gateway Logs',
      command: 'kubectl logs -n wasmbed -l app=wasmbed-gateway',
      description: 'View gateway logs for connection issues',
      category: 'Logs'
    },
    {
      name: 'System Resources',
      command: 'kubectl top pods -n wasmbed',
      description: 'Check CPU and memory usage of all pods',
      category: 'System'
    },
    {
      name: 'Network Status',
      command: 'kubectl get svc -n wasmbed',
      description: 'List all services and their endpoints',
      category: 'Network'
    },
    {
      name: 'Device Enrollment',
      command: 'kubectl get devices -n wasmbed -o wide',
      description: 'Show detailed device enrollment information',
      category: 'Devices'
    },
    {
      name: 'Application Deployment',
      command: 'kubectl get applications -n wasmbed -o wide',
      description: 'Show detailed application deployment status',
      category: 'Applications'
    },
    {
      name: 'Gateway Configuration',
      command: 'kubectl get gateways -n wasmbed -o wide',
      description: 'Show detailed gateway configuration and settings',
      category: 'Gateways'
    },
    {
      name: 'System Events',
      command: 'kubectl get events -n wasmbed --sort-by=.metadata.creationTimestamp',
      description: 'View recent system events and notifications',
      category: 'System'
    },
    {
      name: 'Certificate Status',
      command: 'kubectl get certificates -n wasmbed',
      description: 'Check TLS certificate status and validity',
      category: 'Security'
    },
    {
      name: 'Secret Management',
      command: 'kubectl get secrets -n wasmbed',
      description: 'List all secrets and their types',
      category: 'Security'
    },
    {
      name: 'ConfigMap Status',
      command: 'kubectl get configmaps -n wasmbed',
      description: 'List all configuration maps and their data',
      category: 'Configuration'
    },
    {
      name: 'Node Status',
      command: 'kubectl get nodes',
      description: 'Check cluster node status and resources',
      category: 'Cluster'
    },
    {
      name: 'Namespace Resources',
      command: 'kubectl get all -n wasmbed',
      description: 'List all resources in wasmbed namespace',
      category: 'Kubernetes'
    },
    {
      name: 'WASM Runtime Status',
      command: 'kubectl get pods -n wasmbed -l app=wasmbed-wasm-runtime',
      description: 'Check WebAssembly runtime pod status',
      category: 'Runtime'
    }
  ];

  const categories = [...new Set(predefinedCommands.map(cmd => cmd.category))];

  useEffect(() => {
    // Add welcome message
    addOutput('Welcome to Wasmbed Terminal', 'info');
    addOutput('Type a command or select from predefined commands below', 'info');
    addOutput('', 'text');
  }, []);

  useEffect(() => {
    // Auto-scroll to bottom
    if (outputRef.current) {
      outputRef.current.scrollTop = outputRef.current.scrollHeight;
    }
  }, [output]);

  const addOutput = (text, type = 'text') => {
    const timestamp = new Date().toLocaleTimeString();
    setOutput(prev => [...prev, { text, type, timestamp }]);
  };

  const executeCommand = async (cmd = command) => {
    if (!cmd.trim()) {
      message.warning('Please enter a command');
      return;
    }

    // Security: Only allow predefined commands
    const allowedCommand = predefinedCommands.find(c => c.command === cmd.trim());
    if (!allowedCommand) {
      addOutput(`Security Error: Command "${cmd}" is not allowed. Use predefined commands only.`, 'error');
      return;
    }

    setLoading(true);
    addOutput(`$ ${cmd}`, 'command');

    try {
      // Execute real system commands via secure API with timeout
      const data = await apiPost('/api/v1/terminal/execute', { 
        command: cmd.trim(),
        commandId: allowedCommand.id || allowedCommand.name
      }, 30000);

      if (data.success) {
        // Show the actual output from the command
        const output = data.output || data.result || '';
        if (output.trim()) {
          addOutput(output, 'output');
        } else {
          addOutput('Command executed successfully (no output)', 'info');
        }
      } else {
        addOutput(`Error: ${data.error || 'Command execution failed'}`, 'error');
      }
    } catch (error) {
      if (error.message.includes('timeout')) {
        addOutput(`Timeout Error: Command execution timed out after 30 seconds.`, 'error');
      } else if (error.message.includes('Unexpected token')) {
        addOutput(`Connection Error: Backend service not responding properly. Please check infrastructure status.`, 'error');
      } else {
        addOutput(`Error: ${error.message}`, 'error');
      }
    } finally {
      setLoading(false);
    }
  };

  // Legacy command handling for backward compatibility
  const executeLegacyCommand = async (cmd = command) => {
    if (!cmd.trim()) {
      message.warning('Please enter a command');
      return;
    }

    setLoading(true);
    addOutput(`$ ${cmd}`, 'command');

    try {
      // Simulate command execution with real API calls
      let result = '';
      
      if (cmd.includes('kubectl get devices')) {
        const response = await fetch('/api/v1/devices');
        if (response.ok) {
          const data = await response.json();
          const devices = data.devices || [];
          if (devices.length === 0) {
            result = 'No devices found in wasmbed namespace.';
          } else {
            result = `NAME                    STATUS    GATEWAY    ARCHITECTURE    AGE\n`;
            devices.forEach(device => {
              result += `${device.name.padEnd(20)} ${device.status.toUpperCase().padEnd(8)} ${device.gateway.padEnd(10)} ${device.architecture.padEnd(15)} ${device.age || 'unknown'}\n`;
            });
          }
        } else {
          result = `Error: ${response.status} ${response.statusText}`;
        }
      } else if (cmd.includes('kubectl get applications')) {
        const response = await fetch('/api/v1/applications');
        if (response.ok) {
          const data = await response.json();
          const applications = data.applications || [];
          if (applications.length === 0) {
            result = 'No applications found in wasmbed namespace.';
          } else {
            result = `NAME                    STATUS    TARGET_DEVICES    AGE\n`;
            applications.forEach(app => {
              result += `${app.name.padEnd(20)} ${app.status.toUpperCase().padEnd(8)} ${app.target_devices.padEnd(15)} ${app.age || 'unknown'}\n`;
            });
          }
        } else {
          result = `Error: ${response.status} ${response.statusText}`;
        }
      } else if (cmd.includes('kubectl get gateways')) {
        const response = await fetch('/api/v1/gateways');
        if (response.ok) {
          const data = await response.json();
          const gateways = data.gateways || [];
          if (gateways.length === 0) {
            result = 'No gateways found in wasmbed namespace.';
          } else {
            result = `NAME                    STATUS    ENDPOINT           CONNECTED    ENROLLED    ENABLED\n`;
            gateways.forEach(gateway => {
              result += `${gateway.name.padEnd(20)} ${gateway.status.toUpperCase().padEnd(8)} ${gateway.endpoint.padEnd(18)} ${gateway.connected_devices.toString().padEnd(10)} ${gateway.enrolled_devices.toString().padEnd(9)} ${gateway.enabled ? 'true' : 'false'}\n`;
            });
          }
        } else {
          result = `Error: ${response.status} ${response.statusText}`;
        }
      } else if (cmd.includes('curl') && cmd.includes('health')) {
        try {
          const response = await fetch('http://localhost:30460/health');
          if (response.ok) {
            const data = await response.json();
            result = JSON.stringify(data, null, 2);
          } else {
            result = `Error: ${response.status} ${response.statusText}`;
          }
        } catch (e) {
          result = `Error: ${e.message}`;
        }
      } else if (cmd.includes('kubectl get pods')) {
        // Use terminal execute API instead of direct fetch
        result = 'Note: Pods listing is available via the terminal execute API. Use the predefined "System Status" command.';
      } else if (cmd.includes('kubectl get svc')) {
        // Use terminal execute API instead of direct fetch
        result = 'Note: Services listing is available via the terminal execute API. Use the predefined "Network Status" command.';
      } else if (cmd.includes('kubectl top pods')) {
        // Use terminal execute API instead of direct fetch
        result = 'Note: Pod metrics are available via the terminal execute API. Use the predefined "System Resources" command.';
      } else if (cmd.includes('kubectl logs')) {
        // Use infrastructure logs API
        try {
          const response = await fetch('/api/v1/infrastructure/logs');
          if (response.ok) {
            const data = await response.json();
            const logs = data.logs || [];
            if (logs.length === 0) {
              result = 'No logs available.';
            } else {
              result = logs.map(log => `${log.timestamp || ''}\t${log.source || ''}\t${log.message || ''}`).join('\n');
            }
          } else {
            result = `Error: ${response.status} ${response.statusText}`;
          }
        } catch (e) {
          result = `Error: ${e.message}`;
        }
      } else {
        result = `Command not implemented: ${cmd}\n\nAvailable commands:\n${predefinedCommands.map(c => `- ${c.command}`).join('\n')}`;
      }

      addOutput(result, 'output');
    } catch (error) {
      addOutput(`Error: ${error.message}`, 'error');
    } finally {
      setLoading(false);
    }
  };

  const handleCommandSelect = (value) => {
    const selectedCmd = predefinedCommands.find(cmd => cmd.name === value);
    if (selectedCmd) {
      setCommand(selectedCmd.command);
      setSelectedCommand(value);
    }
  };

  const clearOutput = () => {
    setOutput([]);
    addOutput('Terminal cleared', 'info');
  };

  const copyOutput = () => {
    const text = output.map(line => `[${line.timestamp}] ${line.text}`).join('\n');
    navigator.clipboard.writeText(text);
    message.success('Output copied to clipboard');
  };

  const handleKeyPress = (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      executeCommand();
    }
  };

  const handleCommandClick = (cmd) => {
    setCommand(cmd.command);
    setSelectedCommand(cmd.name);
  };

  const getOutputStyle = (type) => {
    switch (type) {
      case 'command':
        return { color: '#60a5fa', fontWeight: 'bold' };
      case 'output':
        return { color: '#e5e7eb', fontFamily: 'monospace' };
      case 'error':
        return { color: '#f87171', fontFamily: 'monospace' };
      case 'info':
        return { color: '#34d399', fontStyle: 'italic' };
      default:
        return { color: '#9ca3af', fontFamily: 'monospace' };
    }
  };

  return (
    <div>
      <Title level={2}>
        <ConsoleSqlOutlined /> System Terminal
      </Title>
      
      <Row gutter={[16, 16]}>
        <Col xs={24} lg={16}>
          <Card 
            title="Command Terminal" 
            size="small"
            extra={
              <Space>
                <Tooltip title="Clear terminal output">
                  <Button 
                    icon={<ClearOutlined />} 
                    onClick={clearOutput}
                    size="small"
                  />
                </Tooltip>
                <Tooltip title="Copy output to clipboard">
                  <Button 
                    icon={<CopyOutlined />} 
                    onClick={copyOutput}
                    size="small"
                  />
                </Tooltip>
              </Space>
            }
          >
            <div
              ref={outputRef}
              style={{
                height: '350px',
                overflowY: 'auto',
                backgroundColor: '#1a1a1a',
                color: '#ffffff',
                padding: '12px',
                borderRadius: '8px',
                fontFamily: 'monospace',
                fontSize: '13px',
                lineHeight: '1.4',
                marginBottom: '12px',
                border: '1px solid #404040'
              }}
            >
              {output.map((line, index) => (
                <div key={index} style={{ marginBottom: '4px' }}>
                  <span style={{ color: '#6b7280', fontSize: '11px' }}>
                    [{line.timestamp}]
                  </span>{' '}
                  <span style={getOutputStyle(line.type)}>
                    {line.text}
                  </span>
                </div>
              ))}
            </div>
            
            <Space.Compact style={{ width: '100%' }}>
              <Input
                value={command}
                onChange={(e) => setCommand(e.target.value)}
                onKeyPress={handleKeyPress}
                placeholder="Select a predefined command from the list..."
                style={{ fontFamily: 'monospace' }}
                disabled={loading}
                readOnly
              />
              <Button
                type="primary"
                icon={<PlayCircleOutlined />}
                onClick={() => executeCommand()}
                loading={loading}
                disabled={!command.trim()}
              >
                Execute
              </Button>
            </Space.Compact>
            <div style={{ marginTop: '8px', fontSize: '12px', color: '#666' }}>
              <InfoCircleOutlined /> Security: Only predefined commands are allowed for execution
            </div>
          </Card>
        </Col>
        
        <Col xs={24} lg={8}>
          <Card title="Predefined Commands" size="small">
            <Space direction="vertical" style={{ width: '100%' }} size="small">
              <Select
                placeholder="Select a command"
                style={{ width: '100%' }}
                value={selectedCommand}
                onChange={handleCommandSelect}
                optionFilterProp="children"
                showSearch
              >
                {categories.map(category => (
                  <Select.OptGroup key={category} label={category}>
                    {predefinedCommands
                      .filter(cmd => cmd.category === category)
                      .map(cmd => (
                        <Option key={cmd.name} value={cmd.name}>
                          {cmd.name}
                        </Option>
                      ))}
                  </Select.OptGroup>
                ))}
              </Select>
              
              <Divider style={{ margin: '12px 0' }} />
              
              <div style={{ maxHeight: '250px', overflowY: 'auto' }}>
                {predefinedCommands.map((cmd, index) => (
                  <div 
                    key={index} 
                    style={{ 
                      marginBottom: '8px',
                      padding: '8px',
                      border: '1px solid #e5e7eb',
                      borderRadius: '6px',
                      cursor: 'pointer',
                      transition: 'all 0.2s',
                      backgroundColor: command === cmd.command ? '#f0f9ff' : '#ffffff'
                    }}
                    onClick={() => handleCommandClick(cmd)}
                    onMouseEnter={(e) => {
                      if (command !== cmd.command) {
                        e.target.style.backgroundColor = '#f8fafc';
                      }
                    }}
                    onMouseLeave={(e) => {
                      if (command !== cmd.command) {
                        e.target.style.backgroundColor = '#ffffff';
                      }
                    }}
                  >
                    <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                      <Text strong style={{ fontSize: '12px' }}>{cmd.name}</Text>
                      <Tag size="small" color="blue">{cmd.category}</Tag>
                    </div>
                    <Text 
                      code 
                      style={{ 
                        fontSize: '10px', 
                        display: 'block', 
                        marginTop: '3px',
                        backgroundColor: '#f1f5f9',
                        padding: '3px 6px',
                        borderRadius: '3px',
                        wordBreak: 'break-all'
                      }}
                    >
                      {cmd.command}
                    </Text>
                    <Text 
                      type="secondary" 
                      style={{ 
                        fontSize: '10px', 
                        display: 'block', 
                        marginTop: '3px' 
                      }}
                    >
                      {cmd.description}
                    </Text>
                  </div>
                ))}
              </div>
            </Space>
          </Card>
          
          <Card title="Quick Actions" size="small" style={{ marginTop: 16 }}>
            <Space direction="vertical" style={{ width: '100%' }} size="small">
              <Button 
                block 
                onClick={() => executeCommand('kubectl get pods -n wasmbed')}
                loading={loading}
              >
                <ReloadOutlined /> Refresh System Status
              </Button>
              <Button 
                block 
                onClick={() => executeCommand('kubectl get devices -n wasmbed')}
                loading={loading}
              >
                <InfoCircleOutlined /> Check Devices
              </Button>
              <Button 
                block 
                onClick={() => executeCommand('kubectl get applications -n wasmbed')}
                loading={loading}
              >
                <InfoCircleOutlined /> Check Applications
              </Button>
            </Space>
          </Card>
        </Col>
      </Row>
    </div>
  );
};

export default Terminal;
