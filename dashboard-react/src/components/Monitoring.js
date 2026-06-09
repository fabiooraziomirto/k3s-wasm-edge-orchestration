import React, { useState, useEffect, useCallback } from 'react';
import {
  Card,
  Row,
  Col,
  Statistic,
  Typography,
  Table,
  Tag,
  Progress,
  Spin,
  Select,
  DatePicker,
  Space,
  Button,
} from 'antd';
import {
  CheckCircleOutlined,
  ExclamationCircleOutlined,
  ClockCircleOutlined,
  ReloadOutlined,
  LineChartOutlined,
  DatabaseOutlined,
  MobileOutlined,
  CloudServerOutlined,
} from '@ant-design/icons';
import dayjs from 'dayjs';
import { apiGet, apiAll, fetchWithTimeout } from '../utils/api';

const { Title } = Typography;
const { RangePicker } = DatePicker;
const { Option } = Select;

const Monitoring = () => {
  const [loading] = useState(false);
  const [metrics, setMetrics] = useState(null);
  const [logs, setLogs] = useState([]);
  const [timeRange, setTimeRange] = useState('1h');
  const [dateRange, setDateRange] = useState([
    dayjs().subtract(1, 'hour'),
    dayjs(),
  ]);

  const fetchMetrics = useCallback(async () => {
    try {
      // Fetch real data from backend APIs with timeout
      const [devicesData, applicationsData, gatewaysData, infraHealthResponse, infraStatusResponse] = await Promise.all([
        apiGet('/api/v1/devices', 10000),
        apiGet('/api/v1/applications', 10000),
        apiGet('/api/v1/gateways', 10000),
        fetchWithTimeout('/api/v1/infrastructure/health', {}, 5000),
        fetchWithTimeout('/api/v1/infrastructure/status', {}, 5000)
      ]);

      let devices = devicesData.devices || [];
      let applications = applicationsData.applications || [];
      let gateways = gatewaysData.gateways || [];
      
      // Normalize device data - ensure consistent field names
      // API returns devices with 'connected' (boolean) and 'device_id', not 'status' and 'id'
      devices = devices.map(d => {
        // Determine status from 'connected' boolean or existing 'status' field
        let status = d.status;
        if (!status) {
          if (d.connected === true) {
            status = 'Connected';
          } else if (d.enrolled === true) {
            status = 'Enrolled';
          } else {
            status = 'Pending';
          }
        }
        
        return {
          ...d,
          id: d.id || d.device_id || d.name,
          device_id: d.device_id || d.id || d.name,
          status: status,
          enrolled: d.enrolled !== undefined ? d.enrolled : (status === 'Enrolled')
        };
      });
      
      // Determine infrastructure status from new endpoints
      let infraStatus = 'unknown';
      let infraComponents = {
        ca: 'unknown',
        secretStore: 'unknown',
        monitoring: 'unknown',
        logging: 'unknown'
      };

      if (infraHealthResponse.ok) {
        try {
          const infraHealthData = await infraHealthResponse.json();
          infraStatus = infraHealthData.status === 'healthy' ? 'active' : 'inactive';
          console.log('[Monitoring] Infrastructure health:', infraHealthData, '-> status:', infraStatus);
        } catch (e) {
          console.warn('Failed to parse infrastructure health:', e);
        }
      } else {
        console.warn('[Monitoring] Infrastructure health response not OK:', infraHealthResponse.status);
      }

      // Try to parse status response even if not OK (might still have valid JSON)
      try {
        let infraStatusData = null;
        // Check if infraStatusResponse is a valid Response object (has json method)
        if (infraStatusResponse && typeof infraStatusResponse.json === 'function') {
          if (infraStatusResponse.ok) {
            infraStatusData = await infraStatusResponse.json();
          } else {
            // Try to parse anyway - might be a valid JSON response with non-200 status
            try {
              const text = await infraStatusResponse.text();
              infraStatusData = JSON.parse(text);
              console.log('[Monitoring] Parsed status response despite non-OK status:', infraStatusData);
            } catch (parseError) {
              console.warn('[Monitoring] Infrastructure status response not OK and not parseable:', infraStatusResponse.status, infraStatusResponse.statusText);
            }
          }
        } else {
          console.warn('[Monitoring] Infrastructure status response is not a valid Response object:', infraStatusResponse);
        }
        
        if (infraStatusData) {
          console.log('[Monitoring] Infrastructure status response:', infraStatusData);
          // Map infrastructure components - handle different response formats
          const components = infraStatusData.components || {};
          console.log('[Monitoring] Infrastructure components:', components);
          infraComponents = {
            // Map from actual response structure - components use 'healthy' status
            ca: components.ca === 'healthy' ? 'active' : (components.ca ? 'inactive' : 'unknown'),
            secretStore: components.secret_store === 'healthy' ? 'active' : (components.secret_store ? 'inactive' : 'unknown'),
            monitoring: components.monitoring === 'healthy' ? 'active' : (components.monitoring ? 'inactive' : 'unknown'),
            logging: components.logging === 'healthy' ? 'active' : (components.logging ? 'inactive' : 'unknown')
          };
          console.log('[Monitoring] Mapped infrastructure components:', infraComponents);
        }
      } catch (e) {
        console.error('Failed to parse infrastructure status:', e);
        console.error('Response:', infraStatusResponse);
      }
      
      // Fallback: if health endpoint says healthy but components are still unknown, assume they are active
      if (infraStatus === 'active' && (infraComponents.ca === 'unknown' || infraComponents.secretStore === 'unknown')) {
        console.log('[Monitoring] Using fallback: setting components to active (health endpoint reported healthy)');
        if (infraComponents.ca === 'unknown') infraComponents.ca = 'active';
        if (infraComponents.secretStore === 'unknown') infraComponents.secretStore = 'active';
        if (infraComponents.monitoring === 'unknown') infraComponents.monitoring = 'active';
        if (infraComponents.logging === 'unknown') infraComponents.logging = 'active';
      }

      // Calculate real metrics from API data
      // Devices now have normalized 'status' field
      const activeDevices = devices.filter(d => d.status === 'Connected').length;
      const enrolledDevices = devices.filter(d => d.status === 'Enrolled' || d.enrolled === true).length;
      const pendingDevices = devices.filter(d => d.status === 'Pending' || d.status === 'Enrolling' || (!d.status && !d.enrolled)).length;
      const failedDevices = devices.filter(d => d.status === 'Failed' || d.status === 'Unreachable').length;
      
      const runningApplications = applications.filter(a => a.status === 'Running').length;
      const deployingApplications = applications.filter(a => a.status === 'Deploying' || a.status === 'Creating').length;
      const failedApplications = applications.filter(a => a.status === 'Failed').length;
      
      // Gateway status: handle both 'Running' and 'Active' status
      const activeGateways = gateways.filter(g => {
        const status = g.status || '';
        return status === 'Running' || status === 'Active' || (g.enabled === true && status !== 'Stopped');
      }).length;
      
      const inactiveGateways = gateways.filter(g => {
        const status = g.status || '';
        return status === 'Stopped' || status === 'Failed' || status === 'Inactive' || (g.enabled === false);
      }).length;
      
      // Calculate max connections from gateway configs (sum of maxDevices or default 50)
      const maxConnections = gateways.reduce((sum, g) => {
        // Try to get maxDevices from config or use default
        const maxDev = g.maxDevices || g.config?.maxDevices || 50;
        return sum + maxDev;
      }, 0);
      
      // Determine system health based on component status
      // System is "Good" if CA and Secret Store are active
      // Gateways are optional - if no gateways exist, system is still "Good" if infrastructure is healthy
      console.log('[Monitoring] Before systemHealth calculation:', {
        'infraComponents.ca': infraComponents.ca,
        'infraComponents.secretStore': infraComponents.secretStore,
        'infraComponents': infraComponents
      });
      const infraHealthy = infraComponents.ca === 'active' && infraComponents.secretStore === 'active';
      const systemHealth = infraHealthy ? 'Good' : 'Degraded';
      console.log('[Monitoring] System health calculation:', {
        infraHealthy,
        systemHealth,
        reason: infraHealthy ? 'CA and Secret Store are active' : `CA: ${infraComponents.ca}, SecretStore: ${infraComponents.secretStore}`
      });
      
      // Debug logging
      console.log('[Monitoring] Metrics calculated:', {
        devices: devices.length,
        activeDevices,
        enrolledDevices,
        pendingDevices,
        failedDevices,
        applications: applications.length,
        runningApplications,
        gateways: gateways.length,
        activeGateways,
        inactiveGateways,
        maxConnections,
        systemHealth,
        infraComponents
      });

      setMetrics({
        systemHealth: systemHealth,
        activeConnections: activeDevices,
        maxConnections: maxConnections || 50, // Default to 50 if no gateways
        totalDevices: devices.length,
        activeDevices: activeDevices,
        totalApplications: applications.length,
        runningApplications: runningApplications,
        gatewayStatus: {
          active: activeGateways,
          inactive: inactiveGateways,
          totalDevices: activeDevices
        },
        deviceStatus: {
          connected: activeDevices,
          enrolled: enrolledDevices,
          pending: pendingDevices,
          failed: failedDevices
        },
        applicationStatus: {
          running: runningApplications,
          deploying: deployingApplications,
          failed: failedApplications
        },
        infrastructureStatus: infraComponents,
        // System metrics are not available - hide or show as N/A
        systemMetrics: {
          cpuUsage: null, // Not available
          memoryUsage: null, // Not available
          diskUsage: null, // Not available
          networkIn: null, // Not available
          networkOut: null // Not available
        }
      });
    } catch (error) {
      console.error('Error fetching metrics:', error);
      // Set empty metrics when backend is not available
      setMetrics({
        systemHealth: 'Unknown',
        activeConnections: 0,
        maxConnections: 0,
        totalDevices: 0,
        activeDevices: 0,
        totalApplications: 0,
        runningApplications: 0,
        gatewayStatus: {
          active: 0,
          inactive: 0,
          totalDevices: 0
        },
        deviceStatus: {
          connected: 0,
          enrolled: 0,
          pending: 0,
          failed: 0
        },
        applicationStatus: {
          running: 0,
          deploying: 0,
          failed: 0
        },
        infrastructureStatus: {
          ca: 'unknown',
          secretStore: 'unknown',
          monitoring: 'unknown',
          logging: 'unknown'
        },
        systemMetrics: {
          cpuUsage: null,
          memoryUsage: null,
          diskUsage: null,
          networkIn: null,
          networkOut: null
        }
      });
    }
  }, []);

  const fetchLogs = useCallback(async () => {
    try {
      // Try to fetch logs from infrastructure service first, then fallback to general logs
      const [infraLogsResponse, generalLogsResponse] = await Promise.all([
        fetchWithTimeout('/api/v1/infrastructure/logs', {}, 5000).catch(() => ({ ok: false })),
        fetchWithTimeout('/api/v1/logs', {}, 5000).catch(() => ({ ok: false }))
      ]);
      
      let apiLogs = [];
      
      if (infraLogsResponse.ok) {
        try {
          const infraLogsData = await infraLogsResponse.json();
          apiLogs = infraLogsData.logs || [];
        } catch (e) {
          console.warn('Failed to parse infrastructure logs:', e);
        }
      }
      
      if (apiLogs.length === 0 && generalLogsResponse.ok) {
        try {
          const logsData = await generalLogsResponse.json();
          apiLogs = logsData.logs || [];
        } catch (e) {
          console.warn('Failed to parse general logs:', e);
        }
      }
      
      // Normalize log format (ensure all logs have required fields)
      const normalizedLogs = apiLogs.map((log, idx) => {
        // Handle both string and object formats
        if (typeof log === 'string') {
          return {
            id: idx + 1,
            timestamp: new Date().toISOString(),
            level: 'INFO',
            component: 'System',
            message: log
          };
        }
        
        // Ensure timestamp is ISO string
        let timestamp = log.timestamp;
        if (typeof timestamp === 'number') {
          timestamp = new Date(timestamp * 1000).toISOString();
        } else if (!timestamp) {
          timestamp = new Date().toISOString();
        }
        
        return {
          id: log.id || idx + 1,
          timestamp: timestamp,
          level: (log.level || 'INFO').toUpperCase(),
          component: log.component || 'System',
          message: log.message || log.toString()
        };
      });
      
      // If we have API logs, use them; otherwise generate system status logs
      if (normalizedLogs.length > 0) {
        setLogs(normalizedLogs);
      } else {
        // Generate system status logs as fallback
        try {
          const [devicesData, applicationsData, gatewaysData] = await apiAll([
            '/api/v1/devices',
            '/api/v1/applications',
            '/api/v1/gateways'
          ], 10000);

          let devices = devicesData.devices || [];
          let applications = applicationsData.applications || [];
          let gateways = gatewaysData.gateways || [];

          const systemLogs = [];
          
          if (gateways.length > 0) {
            systemLogs.push({
              id: 1,
              timestamp: new Date().toISOString(),
              level: 'INFO',
              component: 'Gateway',
              message: `${gateways.length} gateway(s) configured`
            });
          }

          if (devices.length > 0) {
            // Normalize devices for log generation too
            const normalizedDevices = devices.map(d => ({
              ...d,
              status: d.status || (d.connected === true ? 'Connected' : (d.enrolled === true ? 'Enrolled' : 'Pending'))
            }));
            const connectedDevices = normalizedDevices.filter(d => d.status === 'Connected' || d.connected === true).length;
            const enrolledDevices = normalizedDevices.filter(d => d.status === 'Enrolled' || d.enrolled === true).length;
            systemLogs.push({
              id: 2,
              timestamp: new Date(Date.now() - 30000).toISOString(),
              level: 'INFO',
              component: 'Device Controller',
              message: `${connectedDevices} connected, ${enrolledDevices} enrolled out of ${devices.length} total devices`
            });
          }

          if (applications.length > 0) {
            const runningApps = applications.filter(a => a.status === 'Running').length;
            systemLogs.push({
              id: 3,
              timestamp: new Date(Date.now() - 60000).toISOString(),
              level: 'INFO',
              component: 'Application Controller',
              message: `${runningApps}/${applications.length} applications running`
            });
          }

          setLogs(systemLogs);
        } catch (e) {
          console.error('Failed to generate system logs:', e);
          setLogs([]);
        }
      }
    } catch (error) {
      console.error('Error fetching logs:', error);
      // Set empty logs when backend is not available
      setLogs([]);
    }
  }, []);

  useEffect(() => {
    fetchMetrics();
    fetchLogs();
    const interval = setInterval(() => {
      fetchMetrics();
      fetchLogs();
    }, 30000); // Update every 30 seconds
    return () => clearInterval(interval);
  }, [fetchMetrics, fetchLogs]);

  const getLogLevelTag = (level) => {
    const levelConfig = {
      ERROR: { color: 'red', icon: <ExclamationCircleOutlined /> },
      WARN: { color: 'orange', icon: <ExclamationCircleOutlined /> },
      INFO: { color: 'blue', icon: <CheckCircleOutlined /> },
      DEBUG: { color: 'default', icon: <ClockCircleOutlined /> },
    };

    const config = levelConfig[level] || { color: 'default', icon: null };
    return (
      <Tag color={config.color} icon={config.icon}>
        {level}
      </Tag>
    );
  };

  const logColumns = [
    {
      title: 'Timestamp',
      dataIndex: 'timestamp',
      key: 'timestamp',
      render: (timestamp) => dayjs(timestamp).format('YYYY-MM-DD HH:mm:ss'),
      sorter: (a, b) => dayjs(a.timestamp).unix() - dayjs(b.timestamp).unix(),
    },
    {
      title: 'Level',
      dataIndex: 'level',
      key: 'level',
      render: (level) => getLogLevelTag(level),
      filters: [
        { text: 'ERROR', value: 'ERROR' },
        { text: 'WARN', value: 'WARN' },
        { text: 'INFO', value: 'INFO' },
        { text: 'DEBUG', value: 'DEBUG' },
      ],
      onFilter: (value, record) => record.level === value,
    },
    {
      title: 'Component',
      dataIndex: 'component',
      key: 'component',
      filters: [
        { text: 'Gateway', value: 'Gateway' },
        { text: 'Device Controller', value: 'Device Controller' },
        { text: 'Application Controller', value: 'Application Controller' },
        { text: 'Gateway Controller', value: 'Gateway Controller' },
        { text: 'Infrastructure', value: 'Infrastructure' },
      ],
      onFilter: (value, record) => record.component === value,
    },
    {
      title: 'Message',
      dataIndex: 'message',
      key: 'message',
      ellipsis: true,
    },
  ];

  if (loading) {
    return (
      <div style={{ textAlign: 'center', padding: '50px' }}>
        <Spin size="large" />
        <p>Loading monitoring data...</p>
      </div>
    );
  }

  return (
    <div>
      <Title level={2}>System Monitoring</Title>
      
      <div style={{ marginBottom: 24 }}>
        <Space>
          <Select
            value={timeRange}
            onChange={setTimeRange}
            style={{ width: 120 }}
          >
            <Option value="15m">Last 15m</Option>
            <Option value="1h">Last 1h</Option>
            <Option value="6h">Last 6h</Option>
            <Option value="24h">Last 24h</Option>
            <Option value="7d">Last 7d</Option>
          </Select>
          <RangePicker
            value={dateRange}
            onChange={setDateRange}
            showTime
            format="YYYY-MM-DD HH:mm:ss"
          />
          <Button
            icon={<ReloadOutlined />}
            onClick={() => {
              fetchMetrics();
              fetchLogs();
            }}
          >
            Refresh
          </Button>
        </Space>
      </div>

      {metrics && (
        <>
          <Row gutter={[16, 16]} style={{ marginBottom: 24 }}>
            <Col xs={24} sm={12} lg={6}>
              <Card>
                <Statistic
                  title="System Health"
                  value={metrics.systemHealth || 'Unknown'}
                  prefix={<CheckCircleOutlined />}
                  valueStyle={{ 
                    color: metrics.systemHealth === 'Good' ? '#3f8600' : 
                           metrics.systemHealth === 'Degraded' ? '#faad14' : '#cf1322' 
                  }}
                />
                <Progress
                  percent={metrics.systemHealth === 'Good' ? 100 : metrics.systemHealth === 'Degraded' ? 60 : 0}
                  size="small"
                  status={metrics.systemHealth === 'Good' ? 'success' : metrics.systemHealth === 'Degraded' ? 'exception' : 'exception'}
                />
              </Card>
            </Col>
            
            <Col xs={24} sm={12} lg={6}>
              <Card>
                <Statistic
                  title="Active Connections"
                  value={metrics.activeConnections}
                  prefix={<CloudServerOutlined />}
                  valueStyle={{ color: '#1890ff' }}
                />
                <div style={{ marginTop: 8, fontSize: '12px', color: '#666' }}>
                  Max: {metrics.maxConnections}
                </div>
              </Card>
            </Col>
            
            <Col xs={24} sm={12} lg={6}>
              <Card>
                <Statistic
                  title="Total Applications"
                  value={metrics.totalApplications}
                  prefix={<LineChartOutlined />}
                  valueStyle={{ color: '#1890ff' }}
                />
                <div style={{ marginTop: 8, fontSize: '12px', color: '#666' }}>
                  Running: {metrics.runningApplications}
                </div>
              </Card>
            </Col>
            
            <Col xs={24} sm={12} lg={6}>
              <Card>
                <Statistic
                  title="Total Devices"
                  value={metrics.totalDevices}
                  prefix={<DatabaseOutlined />}
                  valueStyle={{ color: '#1890ff' }}
                />
                <div style={{ marginTop: 8, fontSize: '12px', color: '#666' }}>
                  Connected: {metrics.activeDevices}
                </div>
              </Card>
            </Col>
          </Row>

          <Row gutter={[16, 16]} style={{ marginBottom: 24 }}>
            <Col xs={24} lg={12}>
              <Card title="Device Status Distribution" size="small">
                <Row gutter={16}>
                  <Col span={12}>
                    <Statistic
                      title="Connected"
                      value={metrics.deviceStatus?.connected || 0}
                      valueStyle={{ color: '#3f8600' }}
                    />
                  </Col>
                  <Col span={12}>
                    <Statistic
                      title="Enrolled"
                      value={metrics.deviceStatus?.enrolled || 0}
                      valueStyle={{ color: '#1890ff' }}
                    />
                  </Col>
                </Row>
                <Row gutter={16} style={{ marginTop: 16 }}>
                  <Col span={12}>
                    <Statistic
                      title="Pending"
                      value={metrics.deviceStatus?.pending || 0}
                      valueStyle={{ color: '#faad14' }}
                    />
                  </Col>
                  <Col span={12}>
                    <Statistic
                      title="Failed"
                      value={metrics.deviceStatus?.failed || 0}
                      valueStyle={{ color: '#cf1322' }}
                    />
                  </Col>
                </Row>
              </Card>
            </Col>
            
            <Col xs={24} lg={12}>
              <Card title="Application Status Distribution" size="small">
                <Row gutter={16}>
                  <Col span={8}>
                    <Statistic
                      title="Running"
                      value={metrics.applicationStatus?.running || 0}
                      valueStyle={{ color: '#3f8600' }}
                    />
                  </Col>
                  <Col span={8}>
                    <Statistic
                      title="Deploying"
                      value={metrics.applicationStatus?.deploying || 0}
                      valueStyle={{ color: '#722ed1' }}
                    />
                  </Col>
                  <Col span={8}>
                    <Statistic
                      title="Failed"
                      value={metrics.applicationStatus?.failed || 0}
                      valueStyle={{ color: '#cf1322' }}
                    />
                  </Col>
                </Row>
              </Card>
            </Col>
          </Row>

          <Row gutter={[16, 16]} style={{ marginBottom: 24 }}>
            <Col xs={24} lg={12}>
              <Card title="Gateway Status" size="small">
                <Row gutter={16}>
                  <Col span={12}>
                    <Statistic
                      title="Active Gateways"
                      value={metrics.gatewayStatus?.active || 0}
                      valueStyle={{ color: '#3f8600' }}
                    />
                  </Col>
                  <Col span={12}>
                    <Statistic
                      title="Inactive Gateways"
                      value={metrics.gatewayStatus?.inactive || 0}
                      valueStyle={{ color: '#cf1322' }}
                    />
                  </Col>
                </Row>
                <div style={{ marginTop: 16 }}>
                  <Statistic
                    title="Total Devices Connected"
                    value={metrics.gatewayStatus?.totalDevices || 0}
                    prefix={<MobileOutlined />}
                    valueStyle={{ color: '#1890ff' }}
                  />
                </div>
              </Card>
            </Col>
            
            <Col xs={24} lg={12}>
              <Card title="Infrastructure Status" size="small">
                <Row gutter={16}>
                  <Col span={12}>
                    <Statistic
                      title="Certificate Authority"
                      value={metrics.infrastructureStatus?.ca === 'active' ? 'Active' : 'Inactive'}
                      valueStyle={{ 
                        color: metrics.infrastructureStatus?.ca === 'active' ? '#3f8600' : '#cf1322' 
                      }}
                    />
                  </Col>
                  <Col span={12}>
                    <Statistic
                      title="Secret Store"
                      value={metrics.infrastructureStatus?.secretStore === 'active' ? 'Active' : 'Inactive'}
                      valueStyle={{ 
                        color: metrics.infrastructureStatus?.secretStore === 'active' ? '#3f8600' : '#cf1322' 
                      }}
                    />
                  </Col>
                </Row>
                <Row gutter={16} style={{ marginTop: 16 }}>
                  <Col span={12}>
                    <Statistic
                      title="Monitoring"
                      value={metrics.infrastructureStatus?.monitoring === 'active' ? 'Active' : 'Inactive'}
                      valueStyle={{ 
                        color: metrics.infrastructureStatus?.monitoring === 'active' ? '#3f8600' : '#cf1322' 
                      }}
                    />
                  </Col>
                  <Col span={12}>
                    <Statistic
                      title="Logging"
                      value={metrics.infrastructureStatus?.logging === 'active' ? 'Active' : 'Inactive'}
                      valueStyle={{ 
                        color: metrics.infrastructureStatus?.logging === 'active' ? '#3f8600' : '#cf1322' 
                      }}
                    />
                  </Col>
                </Row>
              </Card>
            </Col>
          </Row>

        </>
      )}

      <Card title="System Logs" size="small">
        <Table
          columns={logColumns}
          dataSource={logs}
          rowKey={(record) => `log-${record.id || record.timestamp || Math.random()}`}
          size="small"
          pagination={{
            pageSize: 20,
            showSizeChanger: true,
            showQuickJumper: true,
            showTotal: (total, range) =>
              `${range[0]}-${range[1]} of ${total} logs`,
          }}
          scroll={{ y: 400 }}
        />
      </Card>
    </div>
  );
};

export default Monitoring;
